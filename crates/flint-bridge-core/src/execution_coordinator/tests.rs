use super::fake::{write, Fake, Mode, PREPARED};
use super::*;
use crate::{
    ffi::{flint_step_fail, flint_step_run, flint_step_succeed},
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

    fn run_posted(&self) {
        assert!(self.fake.run_posted(WAIT), "no step was posted");
    }

    fn finish_dispatcher(&mut self) {
        self.execution_coordinator.schedule.lock().unwrap().take();
        self.schedule.take();
        if let Some(dispatcher) = self.dispatcher.take() {
            crate::tests::join_thread(dispatcher, "execution dispatcher", WAIT);
        }
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
        self.finish_dispatcher();
        self.execution_coordinator.forget_steps();
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
        f.run_posted();
        if asynchronous {
            assert_eq!(f.fake.calls(), ["prepare"], "{case}");
            assert!(f.busy(), "{case}");
            let step = f.fake.take_held().unwrap();
            let completion = thread::spawn(move || {
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
            f.run_posted();
        }
        assert!(!f.busy(), "{case}: run must finish within this posted step");
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
    f.run_posted();
    let result = f.result();
    failed(&result, "preparation_failed");
    assert_eq!(result.traceback.as_deref(), Some("prepare trace"));
    assert_eq!(f.fake.calls(), ["prepare"]);

    *f.fake.prepare.lock().unwrap() = Mode::Complete;
    *f.fake.run.lock().unwrap() = Mode::Fail;
    f.submit();
    f.run_posted();
    failed(&f.result(), "execution_failed");

    *f.fake.run.lock().unwrap() = Mode::Hold;
    f.submit();
    f.run_posted();
    let step = f.fake.take_held().unwrap();
    unsafe { flint_step_fail(step, ptr::null(), c"host_error".as_ptr()) };
    failed(&f.result(), "host_error");
}

#[test]
fn stopping_before_the_posted_step_runs_cancels_without_calling_the_host() {
    let mut f = Fixture::new();
    f.submit();
    let step = f.fake.take_posted(WAIT).unwrap();
    f.stop();
    assert!(!f.busy());
    failed(&f.result(), STOPPED);
    flint_step_run(step);
    assert!(f.fake.calls().is_empty());
}

#[test]
fn stopping_during_preparation_discards_its_value_instead_of_running() {
    let mut f = Fixture::new();
    *f.fake.prepare.lock().unwrap() = Mode::Hold;
    f.submit();
    f.run_posted();
    f.stop();
    assert!(f.busy(), "started preparation must finish first");
    flint_step_succeed(f.fake.take_held().unwrap(), 5);
    assert!(!f.busy());
    failed(&f.result(), STOPPED);
    assert_eq!(f.fake.calls(), ["prepare", "discard 5"]);
}

#[test]
fn a_host_that_cannot_schedule_fails_the_execution() {
    for (case, after_preparation) in [("initial post", false), ("after preparation", true)] {
        let mut f = Fixture::new();
        if after_preparation {
            *f.fake.prepare.lock().unwrap() = Mode::Hold;
            f.submit();
            f.run_posted();
            f.fake.refuse.store(true, Ordering::SeqCst);
            flint_step_succeed(f.fake.take_held().unwrap(), PREPARED);
        } else {
            f.fake.refuse.store(true, Ordering::SeqCst);
            f.submit();
        }
        failed(&f.result(), DROPPED);
        assert!(!f.busy(), "{case}");
        // The result is queued before discard; wait for the dispatcher to finish cleanup.
        f.finish_dispatcher();
        let expected = if after_preparation {
            vec!["prepare".to_string(), format!("discard {PREPARED}")]
        } else {
            vec![]
        };
        assert_eq!(f.fake.calls(), expected, "{case}");
    }
}

#[test]
fn destroying_a_core_releases_lost_steps_and_preserves_its_replacement() {
    use crate::tests::{connected, execute, Backend};

    for (case, prepare, run_posted) in [
        ("posted", Mode::Complete, false),
        ("preparing", Mode::Hold, true),
        ("running", Mode::Complete, true),
    ] {
        let take_step = |fake: &Fake| {
            if run_posted {
                assert!(fake.run_posted(WAIT), "{case}: no posted step");
                fake.take_held().expect("host must hold the step")
            } else {
                fake.take_posted(WAIT).expect("host must receive a step")
            }
        };
        let mut backend = Backend::start_sessions(2);
        let core = connected(&mut backend);
        *core.fake.prepare.lock().unwrap() = prepare;
        backend.send(execute("old"));
        // Take the number away from the helper so its Drop cannot complete it for us.
        let lost = take_step(&core.fake);
        let (coordinator, state) = {
            let steps = STEPS.lock().unwrap();
            let coordinator = &steps.issued[&lost].execution_coordinator;
            (
                Arc::downgrade(coordinator),
                Arc::downgrade(&coordinator.state),
            )
        };
        drop(core); // The helper calls the production flint_bridge_destroy.
        assert!(
            coordinator.upgrade().is_none(),
            "{case}: coordinator retained"
        );
        assert!(
            state.upgrade().is_none(),
            "{case}: execution state retained"
        );

        let replacement = connected(&mut backend);
        *replacement.fake.prepare.lock().unwrap() = prepare;
        backend.send(execute("new"));
        let current = take_step(&replacement.fake);
        assert_ne!(
            current, lost,
            "{case}: reused a destroyed core's step number"
        );
        let calls = replacement.fake.calls();
        assert!(!write(lost, "late output", ""), "{case}");
        flint_step_run(lost);
        flint_step_succeed(lost, PREPARED);
        unsafe { flint_step_fail(lost, ptr::null(), c"late failure".as_ptr()) };
        assert!(
            replacement.busy(),
            "{case}: stale step ended the new execution"
        );
        assert_eq!(replacement.fake.calls(), calls, "{case}");

        if !run_posted {
            flint_step_run(current);
            flint_step_succeed(replacement.fake.take_held().unwrap(), 0);
        } else if prepare == Mode::Hold {
            flint_step_succeed(current, PREPARED);
            assert!(replacement.fake.run_posted(WAIT));
            flint_step_succeed(replacement.fake.take_held().unwrap(), 0);
        } else {
            flint_step_succeed(current, 0);
        }
        assert!(
            !replacement.busy(),
            "{case}: the current step must still complete"
        );
    }
}

#[test]
fn a_completed_preparation_step_cannot_affect_the_running_execution() {
    let mut f = Fixture::new();
    *f.fake.prepare.lock().unwrap() = Mode::Hold;
    *f.fake.run.lock().unwrap() = Mode::Hold;
    f.submit();
    f.run_posted();
    let completed = f.fake.take_held().unwrap();
    flint_step_succeed(completed, PREPARED);
    f.run_posted();
    let running = f.fake.take_held().unwrap();

    assert!(!write(completed, "late preparation output", ""));
    flint_step_succeed(completed, PREPARED);
    unsafe { flint_step_fail(completed, ptr::null(), c"late failure".as_ptr()) };
    flint_step_run(completed);
    assert!(f.busy(), "the running step still owns completion");
    assert_eq!(
        f.fake.calls(),
        ["prepare".to_string(), format!("run {PREPARED}")]
    );

    assert!(write(running, "current output", ""));
    flint_step_succeed(running, 0);
    assert!(!f.busy());
    let messages: [Payload; 2] = f.messages().try_into().expect("output and one result");
    let [Payload::ExecutionOutputUpdate(output), Payload::ExecutionResult(result)] = messages
    else {
        panic!("expected output before the result");
    };
    assert_eq!(output.stdout_delta, "ran\ncurrent output");
    assert_eq!(result.status, ExecutionStatus::Succeeded as i32);
    assert_eq!(result.error, None);
}

#[test]
fn output_is_bounded_ordered_and_flushed_before_the_result() {
    let mut f = Fixture::new();
    *f.fake.run.lock().unwrap() = Mode::Hold;
    f.submit();
    f.run_posted();
    let text = "准备\0🙂".repeat(20000);
    let step = f.fake.held().unwrap();
    assert!(write(step, &text, "错误"));
    flint_step_succeed(f.fake.take_held().unwrap(), 0);
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
    f.run_posted();
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
    f.run_posted();
    let step = f.fake.take_held().unwrap();
    assert!(f.busy());

    f.execution_coordinator.revoke();
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
    flint_step_succeed(step, 0);
    assert!(!f.busy());
    assert_eq!(f.result().status, ExecutionStatus::Succeeded as i32);
    assert_eq!(f.fake.released.load(Ordering::SeqCst), 1);
}
