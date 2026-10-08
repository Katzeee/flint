use super::fake::{write, Fake, Mode, PREPARED};
use super::*;
use crate::{
    ffi::{flint_step_fail, flint_step_succeed, flint_ticket_drop},
    settings::{BridgeSettings, SettingsSnapshot},
};
use flint_contracts::protocol::{
    envelope::Payload, Envelope, ExecutionResult, ExecutionStatus, HostExecuteRequest,
};
use std::{ptr, sync::atomic::Ordering, time::Duration};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

const WAIT: Duration = Duration::from_secs(3);

struct Fixture {
    fake: Arc<Fake>,
    execution_coordinator: Arc<ExecutionCoordinator>,
    state: Arc<Mutex<BridgeState>>,
    schedule: Option<mpsc::Sender<u64>>,
    dispatcher: Option<JoinHandle<()>>,
    outbound: UnboundedSender<Envelope>,
    outputs: UnboundedReceiver<Envelope>,
}

impl Fixture {
    fn new() -> Self {
        let settings = Arc::new(SettingsSnapshot {
            revision: 0,
            settings: BridgeSettings {
                address: "127.0.0.1".into(),
                port: 1,
                name: "test".into(),
                enabled: true,
            },
        });
        let mut state = BridgeState::new(settings.clone());
        state
            .complete_registration(&settings, "instance".into())
            .unwrap();
        let state = Arc::new(Mutex::new(state));
        let (outbound, outputs) = tokio::sync::mpsc::unbounded_channel();
        let fake = Fake::new();
        let (execution_coordinator, schedule, dispatcher) = ExecutionCoordinator::start(
            OwnedExecutionBinding::new(fake.execution_binding()),
            state.clone(),
        )
        .unwrap();
        Self {
            fake,
            execution_coordinator,
            state,
            schedule: Some(schedule),
            dispatcher: Some(dispatcher),
            outbound,
            outputs,
        }
    }

    fn submit(&self) {
        let id = self
            .state
            .lock()
            .unwrap()
            .begin_execution(
                "request".into(),
                HostExecuteRequest {
                    code: "code".into(),
                    ..Default::default()
                },
                self.outbound.clone(),
            )
            .unwrap();
        self.schedule.as_ref().unwrap().send(id).unwrap();
    }

    fn ticket(&self) {
        assert!(self.fake.run_ticket(WAIT), "no ticket was posted");
    }

    fn busy(&self) -> bool {
        self.state.lock().unwrap().busy()
    }

    fn stop(&self) {
        self.state.lock().unwrap().stop();
        self.execution_coordinator.cancel_unstarted();
    }

    fn messages(&mut self) -> Vec<Payload> {
        let mut messages = vec![];
        while let Ok(message) = self.outputs.try_recv() {
            messages.push(message.payload.unwrap());
        }
        messages
    }

    fn result(&mut self) -> ExecutionResult {
        let deadline = std::time::Instant::now() + WAIT;
        loop {
            if let Some(Payload::ExecutionResult(result)) = self.messages().pop() {
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "no result was reported"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.execution_coordinator.revoke();
        self.schedule.take();
        crate::tests::join_thread(
            self.dispatcher.take().unwrap(),
            "execution dispatcher",
            WAIT,
        );
    }
}

fn failed(result: &ExecutionResult, error: &str) {
    assert_eq!(result.status, ExecutionStatus::Failed as i32);
    assert_eq!(result.error.as_deref(), Some(error));
}

#[test]
fn preparation_completion_runs_the_value_and_reports_ordered_output() {
    for (case, asynchronous, result_id, stdout) in [
        ("synchronous", false, PREPARED, "prepared\nran\n"),
        ("asynchronous", true, 9, "preparing\nran\n"),
    ] {
        let mut f = Fixture::new();
        if asynchronous {
            *f.fake.prepare.lock().unwrap() = Mode::Hold;
        }
        f.submit();
        f.ticket();
        if asynchronous {
            assert_eq!(f.fake.calls(), ["prepare"], "{case}");
            assert!(f.busy(), "{case}");
            let step = f.fake.take_held().unwrap() as usize;
            let completion = thread::spawn(move || unsafe {
                let step = step as *mut Step;
                assert!(write(step, "preparing\n", ""));
                flint_step_succeed(step, result_id);
            });
            crate::tests::join_thread(completion, "asynchronous preparation", WAIT);
            assert_eq!(
                f.fake.calls(),
                ["prepare"],
                "completion must wait for the host thread"
            );
            assert!(f.busy());
            f.ticket();
        }
        assert!(!f.busy(), "{case}: run must finish within this ticket");
        assert_eq!(
            f.fake.calls(),
            ["prepare".to_string(), format!("run {result_id}")],
            "{case}"
        );
        assert_eq!(f.fake.take_request().unwrap()["code"], "code", "{case}");
        let messages: [Payload; 2] = f.messages().try_into().expect(case);
        let [Payload::ExecutionOutputUpdate(output), Payload::ExecutionResult(result)] = messages
        else {
            panic!("{case}: expected output before the result");
        };
        assert_eq!(output.stdout_delta, stdout, "{case}");
        assert_eq!(result.status, ExecutionStatus::Succeeded as i32, "{case}");
        assert_eq!(result.error, None, "{case}");
    }
}

#[test]
fn failures_are_classified_by_the_step_that_failed() {
    let mut f = Fixture::new();
    *f.fake.prepare.lock().unwrap() = Mode::Fail;
    f.submit();
    f.ticket();
    let result = f.result();
    failed(&result, "preparation_failed");
    assert_eq!(result.traceback.as_deref(), Some("prepare trace"));
    assert_eq!(f.fake.calls(), ["prepare"]);

    *f.fake.prepare.lock().unwrap() = Mode::Complete;
    *f.fake.run.lock().unwrap() = Mode::Fail;
    f.submit();
    f.ticket();
    failed(&f.result(), "execution_failed");

    *f.fake.run.lock().unwrap() = Mode::Hold;
    f.submit();
    f.ticket();
    let step = f.fake.take_held().unwrap();
    unsafe { flint_step_fail(step, ptr::null(), c"host_error".as_ptr()) };
    failed(&f.result(), "host_error");
}

#[test]
fn stopping_before_the_ticket_runs_cancels_without_calling_the_host() {
    let mut f = Fixture::new();
    f.submit();
    let ticket = f.fake.take_ticket(WAIT).unwrap();
    f.stop();
    assert!(!f.busy());
    failed(&f.result(), STOPPED);
    unsafe { crate::ffi::flint_ticket_run(ticket) };
    assert!(f.fake.calls().is_empty());
}

#[test]
fn stopping_during_preparation_discards_its_value_instead_of_running() {
    let mut f = Fixture::new();
    *f.fake.prepare.lock().unwrap() = Mode::Hold;
    f.submit();
    f.ticket();
    f.stop();
    assert!(f.busy(), "started preparation must finish first");
    unsafe { flint_step_succeed(f.fake.take_held().unwrap(), 5) };
    assert!(!f.busy());
    failed(&f.result(), STOPPED);
    assert_eq!(f.fake.calls(), ["prepare", "discard 5"]);
}

#[test]
fn a_host_that_cannot_schedule_or_run_fails_the_execution() {
    let mut f = Fixture::new();
    f.fake.refuse.store(true, Ordering::SeqCst);
    f.submit();
    failed(&f.result(), DROPPED);
    assert!(!f.busy());

    f.fake.refuse.store(false, Ordering::SeqCst);
    *f.fake.prepare.lock().unwrap() = Mode::Hold;
    f.submit();
    f.ticket();
    unsafe { flint_step_succeed(f.fake.take_held().unwrap(), 3) };
    unsafe { flint_ticket_drop(f.fake.take_ticket(WAIT).unwrap()) };
    failed(&f.result(), DROPPED);
    assert_eq!(f.fake.calls(), ["prepare", "discard 3"]);
}

#[test]
fn output_is_bounded_ordered_and_flushed_before_the_result() {
    let mut f = Fixture::new();
    *f.fake.run.lock().unwrap() = Mode::Hold;
    f.submit();
    f.ticket();
    let text = "准备\0🙂".repeat(20000);
    let step = f.fake.held().unwrap();
    assert!(unsafe { write(step, &text, "错误") });
    unsafe { flint_step_succeed(f.fake.take_held().unwrap(), 0) };
    let (mut stdout, mut stderr, mut sequence) = (String::new(), String::new(), 0);
    let messages = f.messages();
    let (result, updates) = messages.split_last().unwrap();
    assert!(matches!(result, Payload::ExecutionResult(_)));
    for update in updates {
        let Payload::ExecutionOutputUpdate(output) = update else {
            panic!("unexpected message");
        };
        sequence += 1;
        assert_eq!(output.sequence, sequence);
        assert!(output.stdout_delta.len() + output.stderr_delta.len() <= 64 * 1024);
        stdout.push_str(&output.stdout_delta);
        stderr.push_str(&output.stderr_delta);
    }
    assert_eq!(stdout, format!("prepared\nran\n{text}"));
    assert_eq!(stderr, "错误");
}

#[test]
fn revocation_waits_for_the_running_callback_and_silences_later_work() {
    let mut f = Fixture::new();
    let execution_coordinator = f.execution_coordinator.clone();
    let fake = f.fake.clone();
    *f.fake.during_prepare.lock().unwrap() = Some(Box::new(move || {
        execution_coordinator.revoke();
        assert_eq!(fake.released.load(Ordering::SeqCst), 0);
    }));
    f.submit();
    f.ticket();
    f.fake.during_prepare.lock().unwrap().take();
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
    assert_eq!(f.fake.calls(), ["prepare"]);
    failed(&f.result(), STOPPED);

    f.submit();
    failed(&f.result(), STOPPED);
    assert_eq!(f.fake.calls(), ["prepare"]);
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
}

#[test]
fn revocation_releases_only_after_every_concurrent_call_returns() {
    let f = Fixture::new();
    let (entered, entries) = mpsc::channel();
    let mut callers = vec![];
    for _ in 0..2 {
        let execution_coordinator = f.execution_coordinator.clone();
        let entered = entered.clone();
        let (resume, resumed) = mpsc::channel();
        let caller = thread::spawn(move || {
            execution_coordinator.call(|execution_binding| {
                entered.send(()).unwrap();
                resumed.recv_timeout(WAIT).unwrap();
                unsafe { (execution_binding.discard)(execution_binding.context, PREPARED) };
            })
        });
        callers.push((resume, caller));
    }
    for _ in 0..2 {
        entries.recv_timeout(WAIT).unwrap();
    }

    f.execution_coordinator.revoke();
    f.execution_coordinator.revoke();
    assert!(f
        .execution_coordinator
        .call(|_| panic!("revoked call entered"))
        .is_none());
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 0);

    let (resume, caller) = callers.pop().unwrap();
    resume.send(()).unwrap();
    assert_eq!(caller.join().unwrap(), Some(()));
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 0);

    let (resume, caller) = callers.pop().unwrap();
    resume.send(()).unwrap();
    assert_eq!(caller.join().unwrap(), Some(()));
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
    assert_eq!(f.fake.calls(), vec![format!("discard {PREPARED}"); 2]);
}

#[test]
fn an_outstanding_step_does_not_retain_the_execution_binding() {
    let mut f = Fixture::new();
    *f.fake.run.lock().unwrap() = Mode::Hold;
    f.submit();
    f.ticket();
    let step = f.fake.take_held().unwrap();
    assert!(f.busy());

    f.execution_coordinator.revoke();
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
    unsafe { flint_step_succeed(step, 0) };
    assert!(!f.busy());
    assert_eq!(f.result().status, ExecutionStatus::Succeeded as i32);
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
}
