use super::*;
use serde_json::{json, Value};
use std::time::Instant;

struct Core(*mut BridgeCore);

impl Core {
    fn create(config: &str) -> *mut BridgeCore {
        let config = CString::new(config).unwrap();
        unsafe { flint_bridge_create(config.as_ptr()) }
    }
    fn new(port: u16) -> Self {
        let core = Self::create(&config(port));
        assert!(!core.is_null());
        Self(core)
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
    fn poll(&self, timeout: Duration) -> Option<Value> {
        unsafe { take(flint_bridge_poll(self.0, timeout.as_millis() as u32)) }
            .map(|event| serde_json::from_str(&event).unwrap())
    }
    fn submit(&self, command: &str) -> bool {
        let command = CString::new(command).unwrap();
        unsafe { flint_bridge_submit(self.0, command.as_ptr()) }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { flint_bridge_destroy(self.0) }
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
    json!({"host": "python", "address": "127.0.0.1", "port": port, "name": "场景", "runtime_version": "test"})
        .to_string()
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
struct Registry {
    port: u16,
    requests: async_mpsc::UnboundedSender<Envelope>,
    received: mpsc::Receiver<Envelope>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Registry {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (requests, mut pending) = async_mpsc::unbounded_channel::<Envelope>();
        let (forward, received) = mpsc::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(listener).unwrap();
                let (socket, _) = listener.accept().await.unwrap();
                let mut heartbeat = Framed::new(socket, EnvelopeCodec::default());
                let register = heartbeat.next().await.unwrap().unwrap();
                assert!(matches!(
                    register.payload,
                    Some(Payload::RegisterInstance(_))
                ));
                heartbeat.send(accepted(register.request_id)).await.unwrap();
                tokio::spawn(async move {
                    while let Some(Ok(message)) = heartbeat.next().await {
                        if heartbeat.send(accepted(message.request_id)).await.is_err() {
                            break;
                        }
                    }
                });
                let (socket, _) = listener.accept().await.unwrap();
                let mut execution = Framed::new(socket, EnvelopeCodec::default());
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
            });
        });
        Self {
            port,
            requests,
            received,
            thread: Some(thread),
        }
    }
    fn send(&self, envelope: Envelope) {
        self.requests.send(envelope).unwrap();
    }
    fn receive(&mut self) -> Envelope {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match self.received.recv_timeout(Duration::from_millis(50)) {
                Ok(envelope) => return envelope,
                Err(_) => {
                    self.propagate_panic();
                    assert!(Instant::now() < deadline, "registry received nothing");
                }
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

fn connected(registry: &mut Registry) -> Core {
    let core = Core::new(registry.port);
    wait_until(|| {
        registry.propagate_panic();
        core.connected()
    });
    core
}

#[test]
fn create_rejects_invalid_configuration() {
    assert!(unsafe { flint_bridge_create(ptr::null()) }.is_null());
    let valid = config(unused_port());
    let mut unknown: Value = serde_json::from_str(&valid).unwrap();
    unknown["extra"] = true.into();
    let mut empty_host = unknown.clone();
    empty_host.as_object_mut().unwrap().remove("extra");
    empty_host["host"] = "".into();
    let mut zero_port = empty_host.clone();
    zero_port["host"] = "python".into();
    zero_port["port"] = 0.into();
    for config in [
        "not json".to_string(),
        unknown.to_string(),
        empty_host.to_string(),
        zero_port.to_string(),
    ] {
        assert!(Core::create(&config).is_null(), "accepted {config}");
    }
}

#[test]
fn null_handles_are_ignored() {
    let core = ptr::null_mut();
    unsafe {
        assert!(flint_bridge_poll(core, 0).is_null());
        assert!(!flint_bridge_submit(core, c"{}".as_ptr()));
        assert!(!flint_bridge_connected(core));
        assert!(!flint_bridge_busy(core));
        assert!(flint_bridge_instance_id(core).is_null());
        flint_bridge_reconnect(core);
        flint_bridge_stop(core);
        flint_bridge_destroy(core);
        flint_bridge_string_free(ptr::null_mut());
    }
}

#[test]
fn unregistered_bridge_is_idle_and_rejects_commands() {
    let core = Core::new(unused_port());
    assert!(!core.connected());
    assert!(!core.busy());
    assert_eq!(core.instance_id(), "");
    assert!(core.poll(Duration::ZERO).is_none());
    assert!(!core.submit(r#"{"kind":"result","request_id":"unknown","succeeded":true}"#));
    assert!(!core.submit("not json"));
    assert!(unsafe { !flint_bridge_submit(core.0, ptr::null()) });
}

#[test]
fn execution_is_delivered_and_its_output_and_result_are_reported() {
    let mut registry = Registry::start();
    let core = connected(&mut registry);
    assert_eq!(core.instance_id(), "instance-1");
    registry.send(execute("request-1"));
    let event = core.poll(Duration::from_secs(10)).unwrap();
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
    assert!(!core.submit(r#"{"kind":"output","request_id":"other","stdout":"x","stderr":""}"#));
    assert!(core.submit(r#"{"kind":"output","request_id":"request-1","stdout":"","stderr":""}"#));
    assert!(
        core.submit(r#"{"kind":"output","request_id":"request-1","stdout":"你好\n","stderr":""}"#)
    );
    assert!(core.submit(r#"{"kind":"result","request_id":"request-1","succeeded":true}"#));
    assert!(!core.busy());

    let output = registry.receive();
    assert_eq!(output.request_id, "request-1");
    let Some(Payload::ExecutionOutputUpdate(update)) = output.payload else {
        panic!("expected output update, got {output:?}");
    };
    assert_eq!(update.sequence, 1);
    assert_eq!(update.stdout_delta, "你好\n");
    let result = registry.receive();
    let Some(Payload::ExecutionResult(result)) = result.payload else {
        panic!("expected execution result, got {result:?}");
    };
    assert_eq!(result.execution_id, "execution-request-1");
    assert_eq!(result.status, ExecutionStatus::Succeeded as i32);
    assert!(!core.submit(r#"{"kind":"result","request_id":"request-1","succeeded":true}"#));
}

#[test]
fn overlapping_execution_is_answered_as_instance_busy() {
    let mut registry = Registry::start();
    let core = connected(&mut registry);
    registry.send(execute("request-1"));
    assert!(core.poll(Duration::from_secs(10)).is_some());
    registry.send(execute("request-2"));
    let rejected = registry.receive();
    assert_eq!(rejected.request_id, "request-2");
    let Some(Payload::ExecutionResult(result)) = rejected.payload else {
        panic!("expected execution result, got {rejected:?}");
    };
    assert_eq!(result.status, ExecutionStatus::Failed as i32);
    assert_eq!(result.error.as_deref(), Some("instance_busy"));
    assert!(core.poll(Duration::from_millis(100)).is_none());
    assert!(core.busy());
}
