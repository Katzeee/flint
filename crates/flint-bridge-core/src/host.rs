//! The core's only calls into host code. None of them run on the network thread.
use crate::{
    execution::{Failure, Outbound, Stage, DROPPED, STOPPED},
    state::BridgeState,
};
use std::{
    ffi::{c_char, CString},
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
};
use tokio::sync::mpsc::UnboundedSender;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FlintHost {
    pub context: usize,
    pub post: unsafe extern "C" fn(usize, *mut Ticket) -> bool,
    pub prepare: unsafe extern "C" fn(usize, *const c_char, *mut Step),
    pub run: unsafe extern "C" fn(usize, usize, *mut Step),
    pub discard: unsafe extern "C" fn(usize, usize),
    pub release: unsafe extern "C" fn(usize),
}

/// Owns the host registration and releases it when its last caller is done.
pub(crate) struct OwnedHost(FlintHost);

impl OwnedHost {
    pub(crate) fn new(host: FlintHost) -> Self {
        Self(host)
    }
}

impl Drop for OwnedHost {
    fn drop(&mut self) {
        unsafe { (self.0.release)(self.0.context) }
    }
}

pub(crate) struct Host {
    callbacks: Mutex<Option<Arc<OwnedHost>>>,
    state: Arc<Mutex<BridgeState>>,
    outbound: UnboundedSender<Outbound>,
    schedule: Mutex<Option<mpsc::Sender<u64>>>,
}

/// A one-shot request to continue an execution on the host's thread.
pub struct Ticket {
    host: Arc<Host>,
    execution: u64,
}

/// One completion owed by the host for a prepare or run call.
pub struct Step {
    host: Arc<Host>,
    execution: u64,
    /// Preparing or Running when this call starts; does not track later transitions.
    stage: Stage,
}

enum Action {
    Prepare(CString),
    Run(usize),
    Cancel,
}

impl Host {
    /// Returns the host and the sender the network runtime uses to schedule
    /// admitted executions. The dispatcher thread ends after both senders close.
    pub(crate) fn start(
        host: OwnedHost,
        state: Arc<Mutex<BridgeState>>,
        outbound: UnboundedSender<Outbound>,
    ) -> Result<(Arc<Self>, mpsc::Sender<u64>, JoinHandle<()>), String> {
        let (schedule, scheduled) = mpsc::channel();
        let host = Arc::new(Self {
            callbacks: Mutex::new(Some(Arc::new(host))),
            state,
            outbound,
            schedule: Mutex::new(Some(schedule.clone())),
        });
        let dispatcher = host.clone();
        let thread = thread::Builder::new()
            .name("flint-execution-dispatch".into())
            .spawn(move || {
                for id in scheduled {
                    dispatcher.post(id);
                }
            })
            .map_err(|error| {
                host.revoke();
                format!("Cannot start the execution dispatcher: {error}")
            })?;
        Ok((host, schedule, thread))
    }

    fn call<R>(&self, call: impl FnOnce(&FlintHost) -> R) -> Option<R> {
        let host = self.callbacks.lock().unwrap().as_ref()?.clone();
        Some(call(&host.0))
    }

    /// Rejects new calls; release follows the last call already admitted.
    pub(crate) fn revoke(&self) {
        self.schedule.lock().unwrap().take();
        let host = self.callbacks.lock().unwrap().take();
        // Release may call back into the core, so drop outside the lock.
        drop(host);
    }

    fn schedule(&self, id: u64) {
        if let Some(schedule) = self.schedule.lock().unwrap().as_ref() {
            let _ = schedule.send(id);
        }
    }

    fn post(self: &Arc<Self>, id: u64) {
        let ticket = Box::into_raw(Box::new(Ticket {
            host: self.clone(),
            execution: id,
        }));
        let posted = self.call(|host| unsafe { (host.post)(host.context, ticket) });
        if posted != Some(true) {
            drop(unsafe { Box::from_raw(ticket) });
            self.cancel(id, if posted.is_none() { STOPPED } else { DROPPED });
        }
    }

    /// Ends an execution whose code has not started, returning its prepared value.
    fn cancel(&self, id: u64, error: &str) {
        let prepared = {
            let mut state = self.state.lock().unwrap();
            let prepared = match state.execution_mut(id).map(|execution| execution.stage) {
                Some(Stage::Scheduled) => None,
                Some(Stage::Prepared(prepared)) => Some(prepared),
                _ => return,
            };
            state.finish_execution(id, Err(Failure::new(error)), &self.outbound);
            prepared
        };
        if let Some(prepared) = prepared {
            self.call(|host| unsafe { (host.discard)(host.context, prepared) });
        }
    }

    pub(crate) fn cancel_unstarted(&self) {
        let active = self
            .state
            .lock()
            .unwrap()
            .execution
            .as_ref()
            .map(|execution| execution.id);
        if let Some(id) = active {
            self.cancel(id, STOPPED);
        }
    }

    fn step(self: &Arc<Self>, id: u64, stage: Stage) -> *mut Step {
        Box::into_raw(Box::new(Step {
            host: self.clone(),
            execution: id,
            stage,
        }))
    }

    fn advance(self: &Arc<Self>, id: u64) {
        loop {
            let action = {
                let mut state = self.state.lock().unwrap();
                let stopped = state.stopped();
                let Some(execution) = state.execution_mut(id) else {
                    return;
                };
                let stage = execution.stage;
                match stage {
                    Stage::Scheduled | Stage::Prepared(_) if stopped => Action::Cancel,
                    Stage::Scheduled => {
                        execution.stage = Stage::Preparing;
                        Action::Prepare(
                            CString::new(serde_json::to_string(&execution.request).unwrap())
                                .expect("JSON escapes NUL"),
                        )
                    }
                    Stage::Prepared(prepared) => {
                        execution.stage = Stage::Running;
                        Action::Run(prepared)
                    }
                    _ => return,
                }
            };
            match action {
                Action::Cancel => return self.cancel(id, STOPPED),
                Action::Prepare(request) => {
                    let step = self.step(id, Stage::Preparing);
                    let called = self.call(|host| unsafe {
                        (host.prepare)(host.context, request.as_ptr(), step)
                    });
                    if called.is_none() {
                        drop(unsafe { Box::from_raw(step) });
                        self.state.lock().unwrap().finish_execution(
                            id,
                            Err(Failure::new(STOPPED)),
                            &self.outbound,
                        );
                        return;
                    }
                    let mut state = self.state.lock().unwrap();
                    match state.execution_mut(id) {
                        Some(execution) if matches!(execution.stage, Stage::Prepared(_)) => {}
                        Some(execution) => {
                            if execution.stage == Stage::Preparing {
                                execution.stage = Stage::Awaiting;
                            }
                            return;
                        }
                        None => return,
                    }
                }
                Action::Run(prepared) => {
                    let step = self.step(id, Stage::Running);
                    let called =
                        self.call(|host| unsafe { (host.run)(host.context, prepared, step) });
                    if called.is_none() {
                        drop(unsafe { Box::from_raw(step) });
                        self.state.lock().unwrap().finish_execution(
                            id,
                            Err(Failure::new(STOPPED)),
                            &self.outbound,
                        );
                    }
                    return;
                }
            }
        }
    }

    fn complete(&self, step: Step, outcome: Result<usize, Failure>) {
        let (discard, schedule) = {
            let mut state = self.state.lock().unwrap();
            let stopped = state.stopped();
            let stage = state
                .execution_mut(step.execution)
                .map(|execution| execution.stage)
                .filter(|stage| stage.accepts(step.stage));
            match (stage, outcome) {
                (Some(stage @ (Stage::Preparing | Stage::Awaiting)), Ok(prepared)) => {
                    if stopped {
                        state.finish_execution(
                            step.execution,
                            Err(Failure::new(STOPPED)),
                            &self.outbound,
                        );
                        (Some(prepared), false)
                    } else {
                        state.execution_mut(step.execution).unwrap().stage =
                            Stage::Prepared(prepared);
                        (None, stage == Stage::Awaiting)
                    }
                }
                (None, Ok(prepared)) if step.stage == Stage::Preparing => (Some(prepared), false),
                (Some(_), outcome) => {
                    state.finish_execution(step.execution, outcome.map(|_| ()), &self.outbound);
                    (None, false)
                }
                (None, _) => (None, false),
            }
        };
        if let Some(prepared) = discard {
            self.call(|host| unsafe { (host.discard)(host.context, prepared) });
        }
        if schedule {
            self.schedule(step.execution);
        }
    }
}

impl Ticket {
    pub(crate) fn run(self) {
        self.host.advance(self.execution);
    }

    pub(crate) fn drop_unrun(self) {
        self.host.cancel(self.execution, DROPPED);
    }
}

impl Step {
    pub(crate) fn output(&self, stdout: &str, stderr: &str) -> bool {
        let mut state = self.host.state.lock().unwrap();
        let current = state
            .execution_mut(self.execution)
            .is_some_and(|execution| execution.stage.accepts(self.stage));
        current && state.buffer_output(self.execution, stdout, stderr, &self.host.outbound)
    }

    pub(crate) fn succeed(self, prepared: usize) {
        let host = self.host.clone();
        host.complete(self, Ok(prepared));
    }

    pub(crate) fn fail(self, traceback: Option<String>, error: Option<String>) {
        let host = self.host.clone();
        host.complete(self, Err(Failure { traceback, error }));
    }
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
