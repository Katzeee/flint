use super::*;
use crate::host::{
    fake::{write, Fake, Mode},
    OwnedHost,
};
use crate::settings::ApplyResult;
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::time::Instant;
use std::{
    ffi::{c_char, CStr, CString},
    sync::{mpsc, Arc},
    thread,
    time::Duration,
};
use tokio::sync::mpsc as async_mpsc;
use uuid::Uuid;

/// A core whose host runs posted tickets on the test thread and holds each run.
struct Core(*mut BridgeCore, Arc<Fake>);

impl Core {
    fn new(port: u16) -> Self {
        let options = serde_json::from_str(&config(port)).unwrap();
        let fake = Fake::new();
        *fake.run.lock().unwrap() = Mode::Hold;
        let core = BridgeCore::new_for_test(
            options,
            OwnedHost::new(fake.callbacks()),
            &Uuid::new_v4().simple().to_string(),
        )
        .unwrap();
        Self(Box::into_raw(Box::new(core)), fake)
    }
    fn connected(&self) -> bool {
        unsafe { flint_bridge_connected(self.0) }
    }
    fn busy(&self) -> bool {
        unsafe { flint_bridge_busy(self.0) }
    }
    fn instance_id(&self) -> String {
        unsafe { take(flint_bridge_instance_id(self.0)) }.unwrap()
    }
    fn status(&self) -> Value {
        serde_json::from_str(&unsafe { take(flint_bridge_status_json(self.0)) }.unwrap()).unwrap()
    }
    fn wait_request(&self, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            self.1.run_ticket(Duration::from_millis(1));
            if let Some(request) = self.1.take_request() {
                return Some(request);
            }
            if Instant::now() >= deadline {
                return None;
            }
        }
    }
    fn write(&self, stdout: &str, stderr: &str) -> bool {
        self.1
            .held()
            .is_some_and(|step| unsafe { write(step, stdout, stderr) })
    }
    fn finish(&self) -> bool {
        let Some(step) = self.1.take_held() else {
            return false;
        };
        unsafe { flint_step_succeed(step, 0) };
        true
    }
    fn apply_settings(&self, settings: Value) -> u32 {
        let settings = CString::new(settings.to_string()).unwrap();
        unsafe { flint_bridge_apply_settings(self.0, settings.as_ptr()) }
    }
    fn reconnect(&self) {
        assert!(unsafe { flint_bridge_reconnect(self.0) });
    }
    fn obstacle(&self) -> Value {
        self.status()["connection"]["obstacle"].clone()
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe {
            flint_bridge_stop(self.0);
            flint_bridge_destroy(self.0);
        }
        // Destruction leaves outstanding steps valid; finishing one has no effect.
        self.finish();
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

fn unused_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
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

/// Accepts one bridge, acknowledges its heartbeats, and relays the execution channel.
struct Backend {
    port: u16,
    registered: mpsc::Receiver<RegisterInstance>,
    requests: async_mpsc::UnboundedSender<Envelope>,
    received: mpsc::Receiver<Envelope>,
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
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                for _ in 0..sessions {
                    let (socket, _) = listener.accept().await.unwrap();
                    let mut heartbeat = framed(socket);
                    let register = heartbeat.next().await.unwrap().unwrap();
                    let Some(Payload::RegisterInstance(info)) = register.payload else {
                        panic!("expected instance registration");
                    };
                    registration_tx.send(info).unwrap();
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
                                    if forward.send(message).is_err() {
                                        break;
                                    }
                                }
                                _ => break,
                            },
                        }
                    }
                }
            });
        });
        Self {
            port,
            registered,
            requests,
            received,
            thread: Some(thread),
        }
    }
    fn send(&self, envelope: Envelope) {
        self.requests.send(envelope).unwrap();
    }
    fn registration(&self) -> RegisterInstance {
        self.registered
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
    }
    fn receive(&mut self) -> Envelope {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.received.recv_timeout(Duration::from_millis(50)) {
                Ok(envelope) => return envelope,
                Err(_) => {
                    self.propagate_panic();
                    assert!(Instant::now() < deadline, "backend received nothing");
                }
            }
        }
    }
    /// Every envelope through the next execution result, which is last.
    fn until_result(&mut self) -> Vec<Envelope> {
        let mut received = vec![];
        loop {
            let envelope = self.receive();
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
            if let Err(panic) = self.thread.take().unwrap().join() {
                std::panic::resume_unwind(panic);
            }
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
    // Registration identifiers are open, independently of Flint's built-in HostKind.
    assert_eq!(original.instance_type, "custom-editor");
    let second = Backend::start();
    assert_eq!(
        core.apply_settings(settings(second.port, "新场景", true)),
        ApplyResult::Applied as u32
    );
    wait_until(|| core.connected());
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
fn applying_settings_refuses_to_interrupt_an_active_execution() {
    let mut first = Backend::start();
    let core = connected(&mut first);
    first.registration();
    first.send(execute("request-1"));
    assert!(core.wait_request(Duration::from_secs(10)).is_some());
    let second = Backend::start();
    assert_eq!(
        core.apply_settings(settings(second.port, "changed", true)),
        ApplyResult::Busy as u32
    );
    assert!(core.connected());
    assert!(core.finish());
    first.until_result();
    assert_eq!(
        core.apply_settings(settings(second.port, "changed", true)),
        ApplyResult::Applied as u32
    );
    wait_until(|| core.connected());
    assert_eq!(second.registration().instance_name, "changed");
}

#[test]
fn disabling_and_reenabling_keeps_the_core_available() {
    let mut first = Backend::start();
    let core = connected(&mut first);
    first.registration();
    let second = Backend::start();
    assert_eq!(
        core.apply_settings(settings(second.port, "场景", false)),
        ApplyResult::Applied as u32
    );
    wait_until(|| !core.connected());
    assert_eq!(core.status()["connection"], json!({"state": "disabled"}));
    assert_eq!(core.instance_id(), "");
    assert_eq!(
        core.apply_settings(settings(second.port, "场景", true)),
        ApplyResult::Applied as u32
    );
    wait_until(|| core.connected());
    assert_eq!(second.registration().instance_name, "场景");
}

#[test]
fn manual_reconnect_uses_the_applied_settings_without_reporting_an_obstacle() {
    let mut backend = Backend::start_sessions(2);
    let core = connected(&mut backend);
    let original = backend.registration();
    core.reconnect();
    wait_until(|| core.connected());
    let updated = backend.registration();
    assert_eq!(updated.bridge_id, original.bridge_id);
    assert_eq!(updated.instance_name, original.instance_name);
    assert_eq!(core.status()["connection"]["state"], "connected");
    assert!(core.obstacle().is_null());
}

#[test]
fn reconnect_does_not_forward_previous_executions_to_the_new_channel() {
    let mut backend = Backend::start_sessions(2);
    let core = connected(&mut backend);
    backend.registration();
    backend.send(execute("old"));
    assert!(core.wait_request(Duration::from_secs(10)).is_some());
    core.reconnect();
    wait_until(|| core.connected());
    backend.registration();
    assert!(core.busy());
    assert!(core.write("late", ""));
    assert!(core.finish());
    assert!(!core.busy());
    backend.send(execute("new"));
    assert_eq!(
        core.wait_request(Duration::from_secs(10)).unwrap()["request_id"],
        "new"
    );
    assert!(core.finish());
    let received = backend.until_result();
    assert!(received.iter().all(|envelope| envelope.request_id == "new"));
}

#[test]
fn unregistered_bridge_is_idle_and_reports_its_obstacle() {
    let core = Core::new(unused_port());
    wait_until(|| core.obstacle().is_object());
    assert_eq!(core.status()["connection"]["state"], "retrying");
    assert_eq!(core.obstacle()["kind"], "unreachable");
    assert!(core.obstacle()["message"]
        .as_str()
        .is_some_and(|message| !message.is_empty()));
    assert!(!core.connected());
    assert!(!core.busy());
    assert_eq!(core.instance_id(), "");
    assert!(core.wait_request(Duration::ZERO).is_none());
    assert!(!core.finish());
}

#[test]
fn a_backend_that_closes_before_acknowledging_is_a_registration_obstacle() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let closing = thread::spawn(move || {
        let (socket, _) = listener.accept().unwrap();
        drop(socket);
    });
    let core = Core::new(port);
    wait_until(|| core.obstacle().is_object());
    closing.join().unwrap();
    assert_eq!(core.obstacle()["kind"], "registration");
}

#[test]
fn a_registered_session_that_ends_is_lost_and_recovers_on_reconnect() {
    let mut backend = Backend::start();
    let core = connected(&mut backend);
    backend.registration();
    drop(backend);
    wait_until(|| core.obstacle().is_object());
    assert_eq!(core.obstacle()["kind"], "lost");
    assert_eq!(core.instance_id(), "");
    let mut replacement = Backend::start();
    assert_eq!(
        core.apply_settings(settings(replacement.port, "场景", true)),
        ApplyResult::Applied as u32
    );
    wait_until(|| {
        replacement.propagate_panic();
        core.connected()
    });
    assert!(core.obstacle().is_null());
}

#[test]
fn execution_is_delivered_and_its_output_and_result_are_reported() {
    let mut backend = Backend::start();
    let core = connected(&mut backend);
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

#[test]
fn stop_reports_running_host_code_until_it_finishes() {
    let mut backend = Backend::start();
    let core = connected(&mut backend);
    backend.send(execute("request-1"));
    assert!(core.wait_request(Duration::from_secs(10)).is_some());
    assert!(!unsafe { flint_bridge_stop(core.0) });
    assert!(unsafe { flint_bridge_stopped(core.0) });
    assert!(core.busy());
    assert!(core.finish());
    assert!(unsafe { flint_bridge_stop(core.0) });
}

#[test]
fn overlapping_execution_is_answered_as_instance_busy() {
    let mut backend = Backend::start();
    let core = connected(&mut backend);
    backend.send(execute("request-1"));
    assert!(core.wait_request(Duration::from_secs(10)).is_some());
    backend.send(execute("request-2"));
    let rejected = loop {
        let envelope = backend.receive();
        if envelope.request_id == "request-2" {
            break envelope;
        }
    };
    let Some(Payload::ExecutionResult(result)) = rejected.payload else {
        panic!("expected execution result, got {rejected:?}");
    };
    assert_eq!(result.status, ExecutionStatus::Failed as i32);
    assert_eq!(result.error.as_deref(), Some("instance_busy"));
    assert!(core.wait_request(Duration::from_millis(100)).is_none());
    assert!(core.busy());
}
