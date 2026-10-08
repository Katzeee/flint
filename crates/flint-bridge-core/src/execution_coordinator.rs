//! Coordinates execution through the host execution binding, never calling it
//! from the network thread.
use crate::{
    execution::{Failure, Stage, DROPPED, STOPPED},
    execution_binding::{ExecutionBinding, OwnedExecutionBinding},
    state::BridgeState,
};
use std::{
    ffi::CString,
    sync::{mpsc, Arc, Mutex},
    thread::{self, JoinHandle},
};

pub(crate) struct ExecutionCoordinator {
    execution_binding: Mutex<Option<Arc<OwnedExecutionBinding>>>,
    state: Arc<Mutex<BridgeState>>,
    schedule: Mutex<Option<mpsc::Sender<u64>>>,
}

/// A one-shot request to continue an execution on the host's thread.
pub struct Ticket {
    execution_coordinator: Arc<ExecutionCoordinator>,
    execution_id: u64,
}

/// One completion owed by the host for a prepare or run call.
pub struct Step {
    execution_coordinator: Arc<ExecutionCoordinator>,
    execution_id: u64,
}

enum Action {
    Prepare(CString),
    Run(usize),
    Cancel,
}

impl ExecutionCoordinator {
    /// Returns the execution coordinator and the sender the network runtime uses to schedule
    /// admitted executions. The dispatcher thread ends after both senders close.
    pub(crate) fn start(
        execution_binding: OwnedExecutionBinding,
        state: Arc<Mutex<BridgeState>>,
    ) -> Result<(Arc<Self>, mpsc::Sender<u64>, JoinHandle<()>), String> {
        let (schedule, scheduled) = mpsc::channel();
        let execution_coordinator = Arc::new(Self {
            execution_binding: Mutex::new(Some(Arc::new(execution_binding))),
            state,
            schedule: Mutex::new(Some(schedule.clone())),
        });
        let dispatch_execution_coordinator = execution_coordinator.clone();
        let thread = thread::Builder::new()
            .name("flint-execution-dispatch".into())
            .spawn(move || {
                for execution_id in scheduled {
                    dispatch_execution_coordinator.post(execution_id);
                }
            })
            .map_err(|error| {
                execution_coordinator.revoke();
                format!("Cannot start the execution dispatcher: {error}")
            })?;
        Ok((execution_coordinator, schedule, thread))
    }

    fn call<R>(&self, call: impl FnOnce(&ExecutionBinding) -> R) -> Option<R> {
        let execution_binding = self.execution_binding.lock().unwrap().as_ref()?.clone();
        Some(call(&execution_binding.0))
    }

    /// Rejects new calls; release follows the last call already admitted.
    pub(crate) fn revoke(&self) {
        self.schedule.lock().unwrap().take();
        let execution_binding = self.execution_binding.lock().unwrap().take();
        // Release may call back into the core, so drop outside the lock.
        drop(execution_binding);
    }

    fn schedule(&self, execution_id: u64) {
        if let Some(schedule) = self.schedule.lock().unwrap().as_ref() {
            let _ = schedule.send(execution_id);
        }
    }

    fn post(self: &Arc<Self>, execution_id: u64) {
        let ticket = Box::into_raw(Box::new(Ticket {
            execution_coordinator: self.clone(),
            execution_id,
        }));
        let posted = self.call(|execution_binding| unsafe {
            (execution_binding.post)(execution_binding.context, ticket)
        });
        if posted != Some(true) {
            drop(unsafe { Box::from_raw(ticket) });
            self.cancel(
                execution_id,
                if posted.is_none() { STOPPED } else { DROPPED },
            );
        }
    }

    /// Ends an execution whose code has not started and discards its prepared result.
    fn cancel(&self, execution_id: u64, error: &str) {
        let discard = {
            let mut state = self.state.lock().unwrap();
            let discard = match state
                .execution_mut(execution_id)
                .map(|execution| execution.stage)
            {
                Some(Stage::Scheduled) => None,
                Some(Stage::Prepared { result_id }) => Some(result_id),
                _ => return,
            };
            state.finish_execution(execution_id, Err(Failure::new(error)));
            discard
        };
        if let Some(result_id) = discard {
            self.call(|execution_binding| unsafe {
                (execution_binding.discard)(execution_binding.context, result_id)
            });
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
        if let Some(execution_id) = active {
            self.cancel(execution_id, STOPPED);
        }
    }

    fn step(self: &Arc<Self>, execution_id: u64) -> *mut Step {
        Box::into_raw(Box::new(Step {
            execution_coordinator: self.clone(),
            execution_id,
        }))
    }

    fn advance(self: &Arc<Self>, execution_id: u64) {
        loop {
            let action = {
                let mut state = self.state.lock().unwrap();
                let stopped = state.stopped();
                let Some(execution) = state.execution_mut(execution_id) else {
                    return;
                };
                let stage = execution.stage;
                match stage {
                    Stage::Scheduled | Stage::Prepared { .. } if stopped => Action::Cancel,
                    Stage::Scheduled => {
                        execution.stage = Stage::Preparing;
                        Action::Prepare(
                            CString::new(serde_json::to_string(&execution.request).unwrap())
                                .expect("JSON escapes NUL"),
                        )
                    }
                    Stage::Prepared { result_id } => {
                        execution.stage = Stage::Running;
                        Action::Run(result_id)
                    }
                    _ => return,
                }
            };
            match action {
                Action::Cancel => return self.cancel(execution_id, STOPPED),
                Action::Prepare(request) => {
                    let step = self.step(execution_id);
                    let called = self.call(|execution_binding| unsafe {
                        (execution_binding.prepare)(
                            execution_binding.context,
                            request.as_ptr(),
                            step,
                        )
                    });
                    if called.is_none() {
                        drop(unsafe { Box::from_raw(step) });
                        self.state
                            .lock()
                            .unwrap()
                            .finish_execution(execution_id, Err(Failure::new(STOPPED)));
                        return;
                    }
                    let mut state = self.state.lock().unwrap();
                    match state.execution_mut(execution_id) {
                        Some(execution) if matches!(execution.stage, Stage::Prepared { .. }) => {}
                        Some(execution) => {
                            if execution.stage == Stage::Preparing {
                                execution.stage = Stage::Awaiting;
                            }
                            return;
                        }
                        None => return,
                    }
                }
                Action::Run(result_id) => {
                    let step = self.step(execution_id);
                    let called = self.call(|execution_binding| unsafe {
                        (execution_binding.run)(execution_binding.context, result_id, step)
                    });
                    if called.is_none() {
                        drop(unsafe { Box::from_raw(step) });
                        self.state
                            .lock()
                            .unwrap()
                            .finish_execution(execution_id, Err(Failure::new(STOPPED)));
                    }
                    return;
                }
            }
        }
    }

    fn complete(&self, execution_id: u64, outcome: Result<usize, Failure>) {
        let (stopped, resume) = {
            let mut state = self.state.lock().unwrap();
            let stopped = state.stopped();
            let Some(execution) = state.execution_mut(execution_id) else {
                return;
            };
            match (execution.stage, outcome) {
                (stage @ (Stage::Preparing | Stage::Awaiting), Ok(result_id)) => {
                    execution.stage = Stage::Prepared { result_id };
                    (stopped, stage == Stage::Awaiting)
                }
                (_, outcome) => {
                    state.finish_execution(execution_id, outcome.map(|_| ()));
                    return;
                }
            }
        };
        if stopped {
            self.cancel(execution_id, STOPPED);
        } else if resume {
            self.schedule(execution_id);
        }
    }
}

impl Ticket {
    pub(crate) fn run(self) {
        self.execution_coordinator.advance(self.execution_id);
    }

    pub(crate) fn drop_unrun(self) {
        self.execution_coordinator
            .cancel(self.execution_id, DROPPED);
    }
}

impl Step {
    pub(crate) fn output(&self, stdout: &str, stderr: &str) -> bool {
        self.execution_coordinator
            .state
            .lock()
            .unwrap()
            .buffer_output(self.execution_id, stdout, stderr)
    }

    pub(crate) fn succeed(self, result_id: usize) {
        self.execution_coordinator
            .complete(self.execution_id, Ok(result_id));
    }

    pub(crate) fn fail(self, traceback: Option<String>, error: Option<String>) {
        self.execution_coordinator
            .complete(self.execution_id, Err(Failure { traceback, error }));
    }
}

#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod tests;
