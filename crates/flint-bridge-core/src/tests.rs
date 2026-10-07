use super::*;
use crate::claim::TestScope;
use crate::host::{
    fake::{write, Fake, Mode},
    OwnedHost,
};
use crate::settings::ApplyResult;
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    ffi::{c_char, CStr, CString},
    sync::{mpsc, Arc},
    thread,
    time::{Duration, Instant},
};
use tokio::sync::mpsc as async_mpsc;
use tokio_util::sync::CancellationToken;

const WAIT: Duration = Duration::from_secs(10);

pub(crate) fn join_thread(thread: thread::JoinHandle<()>, name: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while !thread.is_finished() {
        if Instant::now() >= deadline {
            if thread::panicking() {
                eprintln!("{name} did not finish within {timeout:?}");
                return;
            }
            panic!("{name} did not finish within {timeout:?}");
        }
        thread::sleep(Duration::from_millis(1));
    }
    if let Err(panic) = thread.join() {
        if thread::panicking() {
            eprintln!("{name} panicked during cleanup");
        } else {
            std::panic::resume_unwind(panic);
        }
    }
}

/// A core whose host runs posted tickets on the test thread and holds each run.
struct Core {
    pointer: *mut BridgeCore,
    fake: Arc<Fake>,
    scope: Option<TestScope>,
}

impl Core {
    fn new(port: u16) -> Self {
        let options = serde_json::from_str(&config(port)).unwrap();
        let fake = Fake::new();
        *fake.run.lock().unwrap() = Mode::Hold;
        let scope = TestScope::new();
        let core =
            BridgeCore::new_for_test(options, OwnedHost::new(fake.callbacks()), scope.name())
                .unwrap();
        Self {
            pointer: Box::into_raw(Box::new(core)),
            fake,
            scope: Some(scope),
        }
    }
    fn connected(&self) -> bool {
        unsafe { flint_bridge_connected(self.pointer) }
    }
    fn busy(&self) -> bool {
        unsafe { flint_bridge_busy(self.pointer) }
    }
    fn instance_id(&self) -> String {
        unsafe { take(flint_bridge_instance_id(self.pointer)) }.unwrap()
    }
    fn status(&self) -> Value {
        serde_json::from_str(&unsafe { take(flint_bridge_status_json(self.pointer)) }.unwrap())
            .unwrap()
    }
    fn wait_request(&self, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            self.fake.run_ticket(Duration::from_millis(1));
            if self.fake.held().is_some() {
                if let Some(request) = self.fake.take_request() {
                    return Some(request);
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
        }
    }
    fn write(&self, stdout: &str, stderr: &str) -> bool {
        self.fake
            .held()
            .is_some_and(|step| unsafe { write(step, stdout, stderr) })
    }
    fn finish(&self) -> bool {
        let Some(step) = self.fake.take_held() else {
            return false;
        };
        unsafe { flint_step_succeed(step, 0) };
        true
    }
    fn apply_settings(&self, settings: Value) -> u32 {
        let settings = CString::new(settings.to_string()).unwrap();
        unsafe { flint_bridge_apply_settings(self.pointer, settings.as_ptr()) }
    }
    fn reconnect(&self) {
        assert!(unsafe { flint_bridge_reconnect(self.pointer) });
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        let pointer = self.pointer as usize;
        let fake = self.fake.clone();
        let scope = self.scope.take();
        let cleanup = thread::spawn(move || {
            let _scope = scope;
            // This worker exclusively owns destruction and the test claim cleanup.
            unsafe {
                let pointer = pointer as *mut BridgeCore;
                flint_bridge_stop(pointer);
                flint_bridge_destroy(pointer);
            }
            if let Some(step) = fake.take_held() {
                unsafe { flint_step_succeed(step, 0) };
            }
        });
        join_thread(cleanup, "core cleanup", WAIT);
    }
}

unsafe fn take(value: *mut c_char) -> Option<String> {
    if value.is_null() {
        return None;
    }
    let text = CStr::from_ptr(value).to_str().unwrap().to_owned();
    flint_bridge_string_free(value);
    Some(text)
}

fn config(port: u16) -> String {
    json!({"host": "custom-editor", "address": "127.0.0.1", "port": port, "name": "场景",
           "runtime_version": "test"})
    .to_string()
}

fn settings(port: u16, name: &str, enabled: bool) -> Value {
    json!({"address":"127.0.0.1","port":port,"name":name,"enabled":enabled})
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !condition() {
        assert!(Instant::now() < deadline, "condition timed out");
        thread::sleep(Duration::from_millis(10));
    }
}

fn accepted(request_id: String) -> Envelope {
    envelope(
        request_id,
        Payload::InstanceAck(InstanceAck {
            success: true,
            instance_id: "instance-1".into(),
            session_token: "token".into(),
            ..Default::default()
        }),
    )
}

fn execute(request_id: &str) -> Envelope {
    envelope(
        request_id.into(),
        Payload::HostExecuteRequest(HostExecuteRequest {
            execution_id: format!("execution-{request_id}"),
            code: "print('你好')".into(),
            workflow_id: "workflow-1".into(),
            filename: Some("scene.py".into()),
            ..Default::default()
        }),
    )
}

/// Accepts bridges, acknowledges heartbeats, and identifies the session receiving each report.
struct Backend {
    port: u16,
    registered: mpsc::Receiver<(usize, RegisterInstance)>,
    requests: async_mpsc::UnboundedSender<Envelope>,
    received: mpsc::Receiver<(usize, Envelope)>,
    session: usize,
    shutdown: CancellationToken,
    thread: Option<thread::JoinHandle<()>>,
}

impl Backend {
    fn start() -> Self {
        Self::start_sessions(1)
    }

    fn start_sessions(sessions: usize) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (requests, mut pending) = async_mpsc::unbounded_channel::<Envelope>();
        let (registration_tx, registered) = mpsc::channel();
        let (forward, received) = mpsc::channel();
        let shutdown = CancellationToken::new();
        let cancelled = shutdown.clone();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                tokio::select! {
                    biased;
                    _ = cancelled.cancelled() => {}
                    _ = async {
                        let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                        for session in 0..sessions {
                            let (socket, _) = listener.accept().await.unwrap();
                            let mut heartbeat = framed(socket);
                            let register = heartbeat.next().await.unwrap().unwrap();
                            let Some(Payload::RegisterInstance(info)) = register.payload else {
                                panic!("expected instance registration");
                            };
                            registration_tx.send((session, info)).unwrap();
                            heartbeat.send(accepted(register.request_id)).await.unwrap();
                            tokio::spawn(async move {
                                while let Some(Ok(message)) = heartbeat.next().await {
                                    if heartbeat.send(accepted(message.request_id)).await.is_err() {
                                        break;
                                    }
                                }
                            });
                            let (socket, _) = listener.accept().await.unwrap();
                            let mut execution = framed(socket);
                            let register = execution.next().await.unwrap().unwrap();
                            let Some(Payload::RegisterExecutionChannel(channel)) = &register.payload else {
                                panic!("expected execution channel registration");
                            };
                            assert_eq!(channel.session_token, "token");
                            execution.send(accepted(register.request_id)).await.unwrap();
                            loop {
                                tokio::select! {
                                    request = pending.recv() => match request {
                                        Some(request) => execution.send(request).await.unwrap(),
                                        None => break,
                                    },
                                    message = execution.next() => match message {
                                        Some(Ok(message)) => {
                                            if forward.send((session, message)).is_err() {
                                                break;
                                            }
                                        }
                                        _ => break,
                                    },
                                }
                            }
                        }
                    } => {}
                }
            });
        });
        Self {
            port,
            registered,
            requests,
            received,
            session: 0,
            shutdown,
            thread: Some(thread),
        }
    }
    fn send(&self, envelope: Envelope) {
        self.requests.send(envelope).unwrap();
    }
    fn registration(&mut self) -> RegisterInstance {
        let (session, registration) = self.registered.recv_timeout(WAIT).unwrap_or_else(|error| {
            self.propagate_panic();
            panic!("backend received no registration: {error}");
        });
        self.session = session;
        registration
    }
    fn receive(&mut self) -> Envelope {
        self.receive_before(Instant::now() + WAIT)
    }
    /// Reports observed on the most recently registered session, including any wrong request ID.
    fn receive_before(&mut self, deadline: Instant) -> Envelope {
        loop {
            self.propagate_panic();
            assert!(
                Instant::now() < deadline,
                "backend received nothing for session {}",
                self.session
            );
            match self.received.recv_timeout(Duration::from_millis(10)) {
                Ok((session, envelope)) if session == self.session => return envelope,
                Ok(_) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.propagate_panic();
                    panic!("backend report channel closed");
                }
            }
        }
    }
    /// Every envelope through the next execution result, within one deadline.
    fn until_result(&mut self) -> Vec<Envelope> {
        let deadline = Instant::now() + WAIT;
        let mut received = vec![];
        loop {
            let envelope = self.receive_before(deadline);
            let done = matches!(envelope.payload, Some(Payload::ExecutionResult(_)));
            received.push(envelope);
            if done {
                return received;
            }
        }
    }
    fn propagate_panic(&mut self) {
        if self
            .thread
            .as_ref()
            .is_some_and(|thread| thread.is_finished())
        {
            join_thread(self.thread.take().unwrap(), "backend", WAIT);
        }
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(thread) = self.thread.take() {
            join_thread(thread, "backend cleanup", WAIT);
        }
    }
}

fn connected(backend: &mut Backend) -> Core {
    let core = Core::new(backend.port);
    wait_until(|| {
        backend.propagate_panic();
        core.connected()
    });
    core
}

#[test]
fn applying_settings_re_registers_on_the_new_endpoint_with_the_new_name() {
    let mut first = Backend::start();
    let core = connected(&mut first);
    let original = first.registration();
    assert_eq!(original.instance_name, "场景");
    assert_eq!(original.instance_type, "custom-editor");

    first.send(execute("active"));
    assert_eq!(core.wait_request(WAIT).unwrap()["request_id"], "active");
    let mut second = Backend::start();
    let replacement = settings(second.port, "新场景", true);
    assert_eq!(
        core.apply_settings(replacement.clone()),
        ApplyResult::Busy as u32
    );
    assert!(core.connected());
    assert_eq!(core.status()["settings"]["name"], "场景");
    assert!(core.finish());
    let completed = first.until_result();
    assert!(completed
        .iter()
        .all(|message| message.request_id == "active"));
    let Some(Payload::ExecutionResult(result)) = &completed.last().unwrap().payload else {
        panic!("expected the active execution result on the original connection");
    };
    assert_eq!(result.status, ExecutionStatus::Succeeded as i32);

    assert_eq!(
        core.apply_settings(replacement),
        ApplyResult::Applied as u32
    );
    wait_until(|| {
        second.propagate_panic();
        core.connected()
    });
    let updated = second.registration();
    assert_eq!(updated.instance_name, "新场景");
    assert_eq!(updated.bridge_id, original.bridge_id);
    assert_eq!(updated.instance_type, original.instance_type);
    let status = core.status();
    assert_eq!(
        status["connection"],
        json!({"state": "connected", "instance_id": "instance-1"})
    );
    assert_eq!(status["settings"]["name"], "新场景");
}

#[test]
fn reconnect_does_not_forward_previous_executions_to_the_new_channel() {
    let mut backend = Backend::start_sessions(2);
    let core = connected(&mut backend);
    backend.registration();
    backend.send(execute("old"));
    assert_eq!(core.wait_request(WAIT).unwrap()["request_id"], "old");

    core.reconnect();
    wait_until(|| {
        backend.propagate_panic();
        core.connected()
    });
    backend.registration();
    assert!(core.busy());
    backend.send(execute("overlap"));
    let rejected = backend.receive();
    assert_eq!(rejected.request_id, "overlap");
    let Some(Payload::ExecutionResult(result)) = rejected.payload else {
        panic!("expected a refusal for the overlapping execution");
    };
    assert_eq!(result.status, ExecutionStatus::Failed as i32);
    assert_eq!(result.error.as_deref(), Some("instance_busy"));
    assert!(core.busy());

    assert!(core.write("late", ""));
    assert!(core.finish());
    assert!(!core.busy());
    backend.send(execute("new"));
    assert_eq!(
        core.wait_request(WAIT).unwrap()["request_id"],
        "new",
        "the refused execution must not run after the old execution finishes"
    );
    assert!(core.finish());
    let received = backend.until_result();
    assert!(received.iter().all(|envelope| envelope.request_id == "new"));
    let Some(Payload::ExecutionResult(result)) = &received.last().unwrap().payload else {
        panic!("expected the new execution result");
    };
    assert_eq!(result.status, ExecutionStatus::Succeeded as i32);
}

#[test]
fn execution_is_delivered_and_its_output_and_result_are_reported() {
    let mut backend = Backend::start();
    let core = connected(&mut backend);
    backend.registration();
    assert_eq!(core.instance_id(), "instance-1");
    backend.send(execute("request-1"));
    let event = core.wait_request(Duration::from_secs(10)).unwrap();
    assert_eq!(
        event,
        json!({
            "request_id": "request-1",
            "workflow_id": "workflow-1",
            "execution_id": "execution-request-1",
            "code": "print('你好')",
            "filename": "scene.py",
        })
    );
    assert!(core.busy());
    assert!(core.write("", ""));
    assert!(core.write("你好\n", ""));
    assert!(core.finish());
    assert!(!core.busy());

    let mut received = backend.until_result();
    assert!(received
        .iter()
        .all(|envelope| envelope.request_id == "request-1"));
    let Some(Payload::ExecutionResult(result)) = received.pop().unwrap().payload else {
        unreachable!()
    };
    let mut stdout = String::new();
    for (sequence, output) in received.into_iter().enumerate() {
        let Some(Payload::ExecutionOutputUpdate(update)) = output.payload else {
            panic!("expected output update, got {output:?}");
        };
        assert_eq!(update.sequence, sequence as u64 + 1);
        stdout.push_str(&update.stdout_delta);
    }
    assert_eq!(stdout, "prepared\nran\n你好\n");
    assert_eq!(result.execution_id, "execution-request-1");
    assert_eq!(result.status, ExecutionStatus::Succeeded as i32);
    assert!(!core.finish());
}
