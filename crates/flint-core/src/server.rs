use crate::{
    store::{now, Store},
    Config,
};
use anyhow::{Context, Result};
use flint_protocol::timing::HEARTBEAT_IDLE_TIMEOUT;
use flint_protocol::{envelope::Payload, *};
use futures_util::{SinkExt, StreamExt};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, Notify},
    task::JoinSet,
};
use tokio_util::{codec::Framed, sync::CancellationToken};
use uuid::Uuid;

type Wire = Framed<TcpStream, EnvelopeCodec>;
fn envelope(id: String, payload: Payload) -> Envelope {
    Envelope {
        protocol_version: PROTOCOL_VERSION,
        request_id: id,
        payload: Some(payload),
    }
}
pub fn failure(code: ErrorCode, message: impl Into<String>) -> Payload {
    Payload::ProtocolError(ProtocolError {
        code: code as i32,
        message: message.into(),
    })
}

struct Session {
    info: InstanceInfo,
    bridge_id: String,
    token: String,
    cancel: CancellationToken,
    sender: Option<mpsc::Sender<Envelope>>,
    exec_generation: String,
    heartbeat: Instant,
}
struct Job {
    instance: String,
    workflow: String,
    execution: String,
    output_sequence: u64,
    result: oneshot::Sender<ExecutionResult>,
    timer: CancellationToken,
}
#[derive(Default)]
struct State {
    sessions: HashMap<String, Session>,
    jobs: HashMap<String, Job>,
    stopping: bool,
}
struct Shared {
    config: Config,
    id: String,
    state: Mutex<State>,
    store: Store,
    shutdown: CancellationToken,
    show_window: Arc<Notify>,
}

#[derive(Clone)]
pub struct BackendHandle(Arc<Shared>);
impl BackendHandle {
    pub fn shutdown_token(&self) -> CancellationToken {
        self.0.shutdown.clone()
    }
    pub fn window_notifications(&self) -> Arc<Notify> {
        self.0.show_window.clone()
    }
    pub fn instances(&self) -> Vec<InstanceInfo> {
        let state = self.0.state.lock().unwrap();
        let mut result: Vec<_> = state.sessions.values().map(|s| s.info.clone()).collect();
        result.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        result
    }
    pub fn status(&self) -> PingResponse {
        PingResponse {
            ready: !self.0.state.lock().unwrap().stopping,
            pid: std::process::id(),
            backend_id: self.0.id.clone(),
            registry_host: self.0.config.registry_host.clone(),
            registry_port: self.0.config.registry_port.into(),
        }
    }
    pub fn request_stop(&self) -> Result<()> {
        let mut state = self.0.state.lock().unwrap();
        anyhow::ensure!(state.jobs.is_empty(), "Executions are still active");
        state.stopping = true;
        self.0.shutdown.cancel();
        Ok(())
    }
}

pub struct Backend {
    handle: BackendHandle,
    control: TcpListener,
    registry: TcpListener,
    _lease: std::fs::File,
}
impl Backend {
    pub async fn bind(config: Config) -> Result<Self> {
        let lease = config.running_lease()?;
        let control = TcpListener::bind((config.host.as_str(), config.port)).await?;
        let registry =
            TcpListener::bind((config.registry_host.as_str(), config.registry_port)).await?;
        let store = Store::open(config.state_dir.join("workflows"))?;
        let shared = Shared {
            config,
            id: Uuid::new_v4().simple().to_string(),
            state: Mutex::new(State::default()),
            store,
            shutdown: CancellationToken::new(),
            show_window: Arc::new(Notify::new()),
        };
        Ok(Self {
            handle: BackendHandle(Arc::new(shared)),
            control,
            registry,
            _lease: lease,
        })
    }
    pub fn handle(&self) -> BackendHandle {
        self.handle.clone()
    }
    pub async fn run(self) -> Result<()> {
        let mut tasks = JoinSet::new();
        let mut sweep = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                _ = self.handle.0.shutdown.cancelled() => break,
                accepted = self.control.accept() => {
                    let (socket, _) = accepted?; let backend = self.handle.clone();
                    tasks.spawn(async move { if let Err(e) = control_connection(backend, socket).await { eprintln!("control connection: {e}"); } });
                }
                accepted = self.registry.accept() => {
                    let (socket, _) = accepted?; let backend = self.handle.clone();
                    tasks.spawn(async move { if let Err(e) = host_connection(backend, socket).await { eprintln!("host connection: {e}"); } });
                }
                _ = sweep.tick() => {
                    let expired: Vec<_> = self.handle.0.state.lock().unwrap().sessions.iter()
                        .filter(|(_,s)| s.heartbeat.elapsed() > HEARTBEAT_IDLE_TIMEOUT).map(|(id,_)| id.clone()).collect();
                    for id in expired { disconnect(&self.handle, &id); }
                }
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
        let ids: Vec<_> = self
            .handle
            .0
            .state
            .lock()
            .unwrap()
            .sessions
            .keys()
            .cloned()
            .collect();
        for id in ids {
            disconnect(&self.handle, &id);
        }
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        Ok(())
    }
}

fn finish(backend: &BackendHandle, request: &str, result: ExecutionResult) {
    let mut state = backend.0.state.lock().unwrap();
    if let Some(job) = state.jobs.remove(request) {
        job.timer.cancel();
        let status = if result.status == ExecutionStatus::Succeeded as i32 {
            "succeeded"
        } else {
            "failed"
        };
        let persisted = backend
            .0
            .store
            .update(&job.workflow, &job.execution, |entry| {
                entry.status = status.into();
                entry.finished_at = Some(now());
                entry.traceback = result.traceback.clone();
                entry.error = result.error.clone();
            });
        let result = match persisted {
            Ok(()) => result,
            Err(e) => ExecutionResult {
                execution_id: job.execution.clone(),
                status: ExecutionStatus::Failed as i32,
                traceback: None,
                error: Some(format!("Could not persist result: {e}")),
            },
        };
        let _ = job.result.send(result);
    }
}
fn disconnect(backend: &BackendHandle, instance: &str) {
    let jobs = {
        let mut state = backend.0.state.lock().unwrap();
        if let Some(session) = state.sessions.remove(instance) {
            session.cancel.cancel();
        }
        state
            .jobs
            .iter()
            .filter(|(_, j)| j.instance == instance)
            .map(|(r, j)| (r.clone(), j.execution.clone()))
            .collect::<Vec<_>>()
    };
    for (request, execution_id) in jobs {
        finish(
            backend,
            &request,
            ExecutionResult {
                execution_id,
                status: ExecutionStatus::Failed as i32,
                traceback: None,
                error: Some("Host disconnected; execution outcome is unknown".into()),
            },
        );
    }
}
async fn read(wire: &mut Wire) -> Result<Envelope> {
    wire.next()
        .await
        .context("connection closed")?
        .map_err(Into::into)
}
async fn control_connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let mut wire = Framed::new(socket, EnvelopeCodec::default());
    let request = tokio::time::timeout(Duration::from_secs(30), read(&mut wire)).await??;
    let id = request.request_id;
    let response = dispatch(&backend, request.payload.unwrap()).await;
    let stop = matches!(response, Payload::StopBackendResponse(_));
    let sent = wire.send(envelope(id, response)).await;
    if stop {
        backend.0.shutdown.cancel();
    }
    sent?;
    Ok(())
}
pub async fn dispatch(backend: &BackendHandle, request: Payload) -> Payload {
    if backend.0.state.lock().unwrap().stopping && !matches!(request, Payload::PingRequest(_)) {
        return failure(ErrorCode::BackendStopping, "Backend is stopping");
    }
    match request {
        Payload::PingRequest(_) => Payload::PingResponse(backend.status()),
        Payload::ShowWindowRequest(_) => {
            backend.0.show_window.notify_one();
            Payload::ShowWindowResponse(ShowWindowResponse { accepted: true })
        }
        Payload::StopBackendRequest(req) => {
            let mut state = backend.0.state.lock().unwrap();
            if req.backend_id != backend.0.id {
                failure(ErrorCode::BackendChanged, "Backend identity changed")
            } else if !state.jobs.is_empty() {
                failure(ErrorCode::BackendBusy, "Executions are still active")
            } else {
                state.stopping = true;
                Payload::StopBackendResponse(StopBackendResponse { stopping: true })
            }
        }
        Payload::ListInstancesRequest(req) => {
            Payload::ListInstancesResponse(ListInstancesResponse {
                instances: backend
                    .instances()
                    .into_iter()
                    .filter(|i| {
                        req.instance_type
                            .as_ref()
                            .map_or(true, |t| t == &i.instance_type)
                    })
                    .collect(),
            })
        }
        Payload::StartWorkflowRequest(req) => {
            match backend.0.store.create(req.name, req.description) {
                Ok(workflow_id) => {
                    Payload::StartWorkflowResponse(StartWorkflowResponse { workflow_id })
                }
                Err(e) => failure(ErrorCode::InternalError, e.to_string()),
            }
        }
        Payload::ExecuteRequest(req) => execute(backend, req).await,
        Payload::GetExecutionRequest(req) => {
            if backend.0.store.load(&req.workflow_id).is_err() {
                return failure(ErrorCode::WorkflowNotFound, "Workflow not found");
            }
            match backend
                .0
                .store
                .execution(&req.workflow_id, &req.execution_id)
            {
                Ok(e) => Payload::GetExecutionResponse(GetExecutionResponse {
                    execution_id: e.execution_id,
                    workflow_id: e.workflow_id,
                    name: e.name,
                    instance_id: e.instance_id,
                    status: match e.status.as_str() {
                        "succeeded" => ExecutionStatus::Succeeded,
                        "failed" => ExecutionStatus::Failed,
                        "pending" => ExecutionStatus::Pending,
                        _ => ExecutionStatus::Running,
                    } as i32,
                    stdout: e.stdout,
                    stderr: e.stderr,
                    started_at: e.started_at,
                    finished_at: e.finished_at,
                    traceback: e.traceback,
                    error: e.error,
                    updated_at: e.updated_at,
                    code: if req.view == ExecutionView::Full as i32 {
                        Some(e.code)
                    } else {
                        None
                    },
                }),
                Err(e) => failure(ErrorCode::ExecutionNotFound, e.to_string()),
            }
        }
        _ => failure(ErrorCode::UnknownRequest, "Not a control request"),
    }
}
async fn execute(backend: &BackendHandle, req: ExecuteRequest) -> Payload {
    let request_id = Uuid::new_v4().simple().to_string();
    let (tx, mut rx) = oneshot::channel();
    let timer = CancellationToken::new();
    let execution_id;
    {
        let mut state = backend.0.state.lock().unwrap();
        if state.stopping {
            return failure(ErrorCode::BackendStopping, "Backend is stopping");
        }
        let Some(session) = state.sessions.get(&req.instance_id) else {
            return failure(ErrorCode::InstanceOffline, "Host is offline");
        };
        let Some(sender) = session.sender.clone() else {
            return failure(ErrorCode::InstanceOffline, "Execution channel is not ready");
        };
        if state.jobs.values().any(|j| j.instance == req.instance_id) {
            return failure(ErrorCode::InstanceBusy, "Instance is busy");
        }
        execution_id = match backend.0.store.append(
            &req.workflow_id,
            &req.instance_id,
            req.code.clone(),
            req.name.clone(),
            request_id.clone(),
        ) {
            Ok(id) => id,
            Err(e) => return failure(ErrorCode::WorkflowNotFound, e.to_string()),
        };
        let command = HostExecuteRequest {
            execution_id: execution_id.clone(),
            code: req.code,
            workflow_id: req.workflow_id.clone(),
            execution_name: Some(req.name),
            filename: req.filename,
        };
        state.jobs.insert(
            request_id.clone(),
            Job {
                instance: req.instance_id.clone(),
                workflow: req.workflow_id,
                execution: execution_id.clone(),
                output_sequence: 0,
                result: tx,
                timer: timer.clone(),
            },
        );
        if sender
            .try_send(envelope(
                request_id.clone(),
                Payload::HostExecuteRequest(command),
            ))
            .is_err()
        {
            drop(state);
            disconnect(backend, &req.instance_id);
            return failure(ErrorCode::ConnectionFailed, "Execution channel is closed");
        }
    }
    let handle = backend.clone();
    let rid = request_id.clone();
    let eid = execution_id.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = timer.cancelled() => {},
            _ = handle.0.shutdown.cancelled() => {},
            _ = tokio::time::sleep(Duration::from_secs(600)) => finish(&handle, &rid, ExecutionResult {
                execution_id: eid, status: ExecutionStatus::Failed as i32, traceback: None,
                error: Some("Execution response timed out; host code may still be running".into()),
            }),
        }
    });
    match tokio::time::timeout(Duration::from_secs(5), &mut rx).await {
        Ok(Ok(result)) => Payload::ExecutionResult(result),
        Ok(Err(_)) => failure(ErrorCode::ConnectionFailed, "Execution response lost"),
        Err(_) => Payload::ExecutionResult(ExecutionResult {
            execution_id,
            status: ExecutionStatus::Running as i32,
            traceback: None,
            error: None,
        }),
    }
}

async fn host_connection(backend: BackendHandle, socket: TcpStream) -> Result<()> {
    let mut wire = Framed::new(socket, EnvelopeCodec::default());
    let first = tokio::time::timeout(Duration::from_secs(30), read(&mut wire)).await??;
    match first.payload.unwrap() {
        Payload::RegisterInstance(req) => {
            anyhow::ensure!(
                req.pid != 0 && !req.bridge_id.is_empty(),
                "Invalid bridge identity"
            );
            let existing = backend
                .0
                .state
                .lock()
                .unwrap()
                .sessions
                .iter()
                .find(|(_, s)| s.bridge_id == req.bridge_id)
                .map(|(id, _)| id.clone());
            if let Some(id) = existing {
                disconnect(&backend, &id);
            }
            let hint: String = req
                .name_hint
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                .take(32)
                .collect();
            let id = format!(
                "{}-{}",
                if hint.is_empty() { "host" } else { &hint },
                &Uuid::new_v4().simple().to_string()[..8]
            );
            let token = Uuid::new_v4().simple().to_string();
            let cancel = CancellationToken::new();
            backend.0.state.lock().unwrap().sessions.insert(
                id.clone(),
                Session {
                    info: InstanceInfo {
                        instance_id: id.clone(),
                        instance_name: req.instance_name,
                        instance_type: req.instance_type,
                        pid: req.pid,
                        runtime_version: req.runtime_version,
                        bridge_version: req.bridge_version,
                        execution_ready: false,
                    },
                    bridge_id: req.bridge_id,
                    token: token.clone(),
                    cancel: cancel.clone(),
                    sender: None,
                    exec_generation: String::new(),
                    heartbeat: Instant::now(),
                },
            );
            let result: Result<()> = async {
                wire.send(envelope(first.request_id, Payload::InstanceAck(InstanceAck { success: true, instance_id: id.clone(), session_token: token, ..Default::default() }))).await?;
                loop {
                    let message = tokio::select! {
                        _ = cancel.cancelled() => break,
                        msg = tokio::time::timeout(HEARTBEAT_IDLE_TIMEOUT, read(&mut wire)) => msg??,
                    };
                    anyhow::ensure!(matches!(message.payload, Some(Payload::Heartbeat(ref h)) if h.instance_id == id), "Invalid heartbeat");
                    if let Some(session) = backend.0.state.lock().unwrap().sessions.get_mut(&id) { session.heartbeat = Instant::now(); }
                    wire.send(envelope(message.request_id, Payload::InstanceAck(InstanceAck { success: true, ..Default::default() }))).await?;
                }
                Ok(())
            }.await;
            disconnect(&backend, &id);
            result
        }
        Payload::RegisterExecutionChannel(req) => {
            let (sender, mut receiver) = mpsc::channel(16);
            let generation = Uuid::new_v4().simple().to_string();
            let cancel = {
                let mut state = backend.0.state.lock().unwrap();
                let session = state
                    .sessions
                    .get_mut(&req.instance_id)
                    .context("Instance not registered")?;
                anyhow::ensure!(
                    session.info.pid == req.pid && session.token == req.session_token,
                    "Execution channel identity mismatch"
                );
                anyhow::ensure!(
                    session.sender.is_none(),
                    "Execution channel already connected"
                );
                session.sender = Some(sender);
                session.exec_generation = generation.clone();
                session.info.execution_ready = true;
                session.cancel.clone()
            };
            let result: Result<()> = async {
                wire.send(envelope(first.request_id, Payload::InstanceAck(InstanceAck { success: true, ..Default::default() }))).await?;
                loop {
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        next = receiver.recv() => { if let Some(message) = next { wire.send(message).await?; } else { break; } },
                        next = wire.next() => {
                            let message = next.context("Execution channel closed")??;
                            process_host_message(&backend, &req.instance_id, message)?;
                        }
                    }
                }
                Ok(())
            }.await;
            let owns = backend
                .0
                .state
                .lock()
                .unwrap()
                .sessions
                .get(&req.instance_id)
                .map_or(false, |s| s.exec_generation == generation);
            if owns {
                disconnect(&backend, &req.instance_id);
            }
            result
        }
        _ => anyhow::bail!("Expected host registration"),
    }
}
fn process_host_message(backend: &BackendHandle, instance: &str, message: Envelope) -> Result<()> {
    let mut state = backend.0.state.lock().unwrap();
    let Some(job) = state.jobs.get_mut(&message.request_id) else {
        return Ok(());
    };
    anyhow::ensure!(
        job.instance == instance,
        "Response belongs to another instance"
    );
    match message.payload.unwrap() {
        Payload::ExecutionOutputUpdate(update) => {
            anyhow::ensure!(
                update.execution_id == job.execution && update.workflow_id == job.workflow,
                "Output identity mismatch"
            );
            if update.sequence <= job.output_sequence {
                return Ok(());
            }
            anyhow::ensure!(
                update.sequence == job.output_sequence + 1,
                "Output sequence gap"
            );
            backend.0.store.update(&job.workflow, &job.execution, |e| {
                e.stdout += &update.stdout_delta;
                e.stderr += &update.stderr_delta;
            })?;
            job.output_sequence = update.sequence;
        }
        Payload::ExecutionResult(result) => {
            anyhow::ensure!(
                result.execution_id == job.execution
                    && matches!(
                        ExecutionStatus::try_from(result.status),
                        Ok(ExecutionStatus::Succeeded | ExecutionStatus::Failed)
                    ),
                "Invalid execution result"
            );
            drop(state);
            finish(backend, &message.request_id, result);
        }
        _ => anyhow::bail!("Unexpected host message"),
    }
    Ok(())
}
