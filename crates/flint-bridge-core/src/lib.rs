//! Host-side connection, wire protocol, and execution coordination.
//!
//! The C ABI exchanges UTF-8 JSON at its boundary. Execute events are copied
//! by the host adapter and completed asynchronously; host APIs stay outside
//! the core and run on the thread selected by that adapter.

use flint_protocol::timing::{HEARTBEAT_ACK_TIMEOUT, HEARTBEAT_INTERVAL};
use flint_protocol::{envelope::Payload, *};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    ffi::{c_char, CStr, CString},
    ptr,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use tokio::{net::TcpStream, sync::mpsc as async_mpsc};
use tokio_util::{codec::Framed, sync::CancellationToken};
use uuid::Uuid;

type Wire = Framed<TcpStream, EnvelopeCodec>;

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    host: String,
    address: String,
    port: u16,
    name: String,
    runtime_version: String,
}

#[derive(Serialize)]
struct ExecuteEvent {
    request_id: String,
    workflow_id: String,
    execution_id: String,
    code: String,
    filename: Option<String>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Command {
    Output {
        request_id: String,
        stdout: String,
        stderr: String,
    },
    Result {
        request_id: String,
        succeeded: bool,
        traceback: Option<String>,
        error: Option<String>,
    },
}

struct Active {
    generation: u64,
    request_id: String,
    workflow_id: String,
    execution_id: String,
    sequence: u64,
}

#[derive(Default)]
struct State {
    connected: bool,
    instance_id: String,
    generation: u64,
    active: Option<Active>,
}

struct Outbound {
    generation: u64,
    envelope: Envelope,
}

pub struct BridgeCore {
    state: Arc<Mutex<State>>,
    events: Mutex<mpsc::Receiver<String>>,
    outbound: async_mpsc::UnboundedSender<Outbound>,
    stop: CancellationToken,
    reconnect: Arc<tokio::sync::Notify>,
    thread: Mutex<Option<thread::JoinHandle<()>>>,
}

fn envelope(request_id: String, payload: Payload) -> Envelope {
    Envelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        payload: Some(payload),
    }
}

async fn read(wire: &mut Wire) -> Result<Envelope, String> {
    match tokio::time::timeout(Duration::from_secs(10), wire.next()).await {
        Ok(Some(Ok(message))) => Ok(message),
        Ok(Some(Err(error))) => Err(error.to_string()),
        Ok(None) => Err("connection closed".into()),
        Err(_) => Err("response timed out".into()),
    }
}

async fn connect(config: &Config) -> Result<Wire, String> {
    let stream = tokio::time::timeout(
        Duration::from_secs(10),
        TcpStream::connect((config.address.as_str(), config.port)),
    )
    .await
    .map_err(|_| "connection timed out".to_string())?
    .map_err(|error| error.to_string())?;
    Ok(Framed::new(stream, EnvelopeCodec::default()))
}

async fn ack(wire: &mut Wire, request_id: &str) -> Result<InstanceAck, String> {
    let response = read(wire).await?;
    if response.request_id != request_id {
        return Err("handshake request ID mismatch".into());
    }
    match response.payload {
        Some(Payload::InstanceAck(ack)) if ack.success => Ok(ack),
        _ => Err("bridge handshake rejected".into()),
    }
}

async fn heartbeat(mut wire: Wire, instance_id: String) -> Result<(), String> {
    loop {
        let request_id = Uuid::new_v4().simple().to_string();
        wire.send(envelope(
            request_id.clone(),
            Payload::Heartbeat(Heartbeat {
                instance_id: instance_id.clone(),
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
        tokio::time::timeout(HEARTBEAT_ACK_TIMEOUT, ack(&mut wire, &request_id))
            .await
            .map_err(|_| "heartbeat acknowledgement timed out".to_string())??;
        tokio::time::sleep(HEARTBEAT_INTERVAL).await;
    }
}

async fn execution(
    mut wire: Wire,
    generation: u64,
    state: Arc<Mutex<State>>,
    events: mpsc::Sender<String>,
    outbound: &mut async_mpsc::UnboundedReceiver<Outbound>,
) -> Result<(), String> {
    loop {
        tokio::select! {
            incoming = wire.next() => {
                let message = incoming.ok_or("execution channel closed")?
                    .map_err(|error| error.to_string())?;
                let Some(Payload::HostExecuteRequest(request)) = message.payload else {
                    return Err("unexpected execution message".into());
                };
                let event = {
                    let mut state = state.lock().unwrap();
                    if state.active.is_some() {
                        None
                    } else {
                        state.active = Some(Active {
                            generation,
                            request_id: message.request_id.clone(),
                            workflow_id: request.workflow_id.clone(),
                            execution_id: request.execution_id.clone(),
                            sequence: 0,
                        });
                        Some(ExecuteEvent {
                            request_id: message.request_id.clone(),
                            workflow_id: request.workflow_id,
                            execution_id: request.execution_id.clone(),
                            code: request.code,
                            filename: request.filename,
                        })
                    }
                };
                if let Some(event) = event {
                    events.send(serde_json::to_string(&event).unwrap())
                        .map_err(|_| "host event receiver closed".to_string())?;
                } else {
                    wire.send(envelope(message.request_id, Payload::ExecutionResult(ExecutionResult {
                        execution_id: request.execution_id,
                        status: ExecutionStatus::Failed as i32,
                        traceback: None,
                        error: Some("instance_busy".into()),
                    }))).await.map_err(|error| error.to_string())?;
                }
            }
            command = outbound.recv() => {
                let command = command.ok_or("outbound channel closed")?;
                if command.generation == generation {
                    wire.send(command.envelope).await.map_err(|error| error.to_string())?;
                }
            }
        }
    }
}

async fn session(
    config: &Config,
    bridge_id: &str,
    state: Arc<Mutex<State>>,
    events: mpsc::Sender<String>,
    outbound: &mut async_mpsc::UnboundedReceiver<Outbound>,
    stop: &CancellationToken,
    reconnect: &tokio::sync::Notify,
) -> Result<(), String> {
    let mut heartbeat_wire = connect(config).await?;
    let request_id = Uuid::new_v4().simple().to_string();
    heartbeat_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterInstance(RegisterInstance {
                pid: std::process::id(),
                name_hint: config.host.clone(),
                instance_name: config.name.clone(),
                instance_type: config.host.clone(),
                bridge_id: bridge_id.into(),
                runtime_version: config.runtime_version.clone(),
                bridge_version: env!("CARGO_PKG_VERSION").into(),
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
    let identity = ack(&mut heartbeat_wire, &request_id).await?;
    if identity.instance_id.is_empty() || identity.session_token.is_empty() {
        return Err("incomplete instance registration".into());
    }
    let mut execution_wire = connect(config).await?;
    let request_id = Uuid::new_v4().simple().to_string();
    execution_wire
        .send(envelope(
            request_id.clone(),
            Payload::RegisterExecutionChannel(RegisterExecutionChannel {
                instance_id: identity.instance_id.clone(),
                pid: std::process::id(),
                session_token: identity.session_token,
            }),
        ))
        .await
        .map_err(|error| error.to_string())?;
    ack(&mut execution_wire, &request_id).await?;
    let generation = {
        let mut state = state.lock().unwrap();
        state.generation += 1;
        state.connected = true;
        state.instance_id = identity.instance_id.clone();
        state.generation
    };
    let heartbeat = heartbeat(heartbeat_wire, identity.instance_id);
    let execution = execution(execution_wire, generation, state.clone(), events, outbound);
    tokio::select! {
        result = heartbeat => result,
        result = execution => result,
        _ = reconnect.notified() => Err("reconnect requested".into()),
        _ = stop.cancelled() => Ok(()),
    }
}

async fn run(
    config: Config,
    state: Arc<Mutex<State>>,
    events: mpsc::Sender<String>,
    mut outbound: async_mpsc::UnboundedReceiver<Outbound>,
    stop: CancellationToken,
    reconnect: Arc<tokio::sync::Notify>,
) {
    let bridge_id = Uuid::new_v4().simple().to_string();
    let mut delay = 0;
    while !stop.is_cancelled() {
        let result = tokio::select! {
            result = session(&config, &bridge_id, state.clone(), events.clone(), &mut outbound, &stop, &reconnect) => result,
            _ = stop.cancelled() => Ok(()),
        };
        state.lock().unwrap().connected = false;
        if stop.is_cancelled() {
            break;
        }
        delay = if result.is_ok() {
            0
        } else {
            (delay * 2 + 1).min(10)
        };
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(delay)) => {},
            _ = stop.cancelled() => break,
        }
    }
}

impl BridgeCore {
    fn new(config: Config) -> Result<Self, String> {
        if config.address.is_empty() || config.host.is_empty() || config.port == 0 {
            return Err("invalid bridge configuration".into());
        }
        let (events_tx, events_rx) = mpsc::channel();
        let (outbound_tx, outbound_rx) = async_mpsc::unbounded_channel();
        let state = Arc::new(Mutex::new(State::default()));
        let stop = CancellationToken::new();
        let reconnect = Arc::new(tokio::sync::Notify::new());
        let thread_state = state.clone();
        let thread_stop = stop.clone();
        let thread_reconnect = reconnect.clone();
        let thread = thread::Builder::new()
            .name("flint-bridge-core".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Tokio runtime");
                runtime.block_on(run(
                    config,
                    thread_state,
                    events_tx,
                    outbound_rx,
                    thread_stop,
                    thread_reconnect,
                ));
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            state,
            events: Mutex::new(events_rx),
            outbound: outbound_tx,
            stop,
            reconnect,
            thread: Mutex::new(Some(thread)),
        })
    }

    fn submit(&self, command: Command) -> bool {
        let mut state = self.state.lock().unwrap();
        let Some(active) = state.active.as_mut() else {
            return false;
        };
        let (generation, envelope) = match command {
            Command::Output {
                request_id,
                stdout,
                stderr,
            } => {
                if request_id != active.request_id {
                    return false;
                }
                if stdout.is_empty() && stderr.is_empty() {
                    return true;
                }
                active.sequence += 1;
                (
                    active.generation,
                    envelope(
                        request_id,
                        Payload::ExecutionOutputUpdate(ExecutionOutputUpdate {
                            workflow_id: active.workflow_id.clone(),
                            execution_id: active.execution_id.clone(),
                            sequence: active.sequence,
                            stdout_delta: stdout,
                            stderr_delta: stderr,
                        }),
                    ),
                )
            }
            Command::Result {
                request_id,
                succeeded,
                traceback,
                error,
            } => {
                if request_id != active.request_id {
                    return false;
                }
                let generation = active.generation;
                let envelope = envelope(
                    request_id,
                    Payload::ExecutionResult(ExecutionResult {
                        execution_id: active.execution_id.clone(),
                        status: if succeeded {
                            ExecutionStatus::Succeeded
                        } else {
                            ExecutionStatus::Failed
                        } as i32,
                        traceback,
                        error,
                    }),
                );
                state.active = None;
                (generation, envelope)
            }
        };
        self.outbound
            .send(Outbound {
                generation,
                envelope,
            })
            .is_ok()
    }

    fn stop(&self) {
        self.stop.cancel();
        if let Some(thread) = self.thread.lock().unwrap().take() {
            let _ = thread.join();
        }
    }
}

unsafe fn input(value: *const c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        CStr::from_ptr(value).to_str().ok().map(str::to_owned)
    }
}

#[no_mangle]
pub extern "C" fn flint_bridge_abi_version() -> u32 {
    1
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_create(config_json: *const c_char) -> *mut BridgeCore {
    let Some(config) =
        input(config_json).and_then(|text| serde_json::from_str::<Config>(&text).ok())
    else {
        return ptr::null_mut();
    };
    match BridgeCore::new(config) {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(_) => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_poll(
    core: *const BridgeCore,
    timeout_ms: u32,
) -> *mut c_char {
    let Some(core) = core.as_ref() else {
        return ptr::null_mut();
    };
    let result = core
        .events
        .lock()
        .unwrap()
        .recv_timeout(Duration::from_millis(timeout_ms as u64));
    match result.ok().and_then(|event| CString::new(event).ok()) {
        Some(event) => event.into_raw(),
        None => ptr::null_mut(),
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_submit(
    core: *const BridgeCore,
    command_json: *const c_char,
) -> bool {
    let Some(core) = core.as_ref() else {
        return false;
    };
    let Some(command) =
        input(command_json).and_then(|text| serde_json::from_str::<Command>(&text).ok())
    else {
        return false;
    };
    core.submit(command)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_connected(core: *const BridgeCore) -> bool {
    core.as_ref()
        .is_some_and(|core| core.state.lock().unwrap().connected)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_busy(core: *const BridgeCore) -> bool {
    core.as_ref()
        .is_some_and(|core| core.state.lock().unwrap().active.is_some())
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_instance_id(core: *const BridgeCore) -> *mut c_char {
    let Some(core) = core.as_ref() else {
        return ptr::null_mut();
    };
    CString::new(core.state.lock().unwrap().instance_id.clone())
        .unwrap()
        .into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_reconnect(core: *const BridgeCore) {
    if let Some(core) = core.as_ref() {
        core.reconnect.notify_one();
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_stop(core: *const BridgeCore) {
    if let Some(core) = core.as_ref() {
        core.stop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_destroy(core: *mut BridgeCore) {
    if !core.is_null() {
        let core = Box::from_raw(core);
        core.stop();
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests;
