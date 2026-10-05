use crate::store::{now, Store};
use anyhow::{Context, Result};
use flint_config::Config;
use flint_protocol::timing::HEARTBEAT_IDLE_TIMEOUT;
use flint_protocol::{envelope::Payload, *};
use futures_util::SinkExt;
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
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod bridge;
mod control;

const FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);

type Wire = flint_protocol::framing::Wire<TcpStream>;

fn failure(code: ErrorCode, message: impl Into<String>) -> Payload {
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
    state: Mutex<State>,
    store: Store,
    shutdown: CancellationToken,
    show_window: Arc<Notify>,
}

#[derive(Clone)]
pub struct BackendHandle(Arc<Shared>);
impl BackendHandle {
    pub fn workflows(&self) -> Result<Vec<crate::store::WorkflowSummary>> {
        self.0.store.list()
    }
    pub fn workflow(&self, id: &str) -> Result<crate::store::Workflow> {
        self.0.store.load(id)
    }
    pub fn config(&self) -> &Config {
        &self.0.config
    }
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
            bridge_address: self.0.config.address.clone(),
            bridge_port: self.0.config.bridge_port.into(),
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
    bridge: TcpListener,
    _lease: std::fs::File,
}
impl Backend {
    pub async fn bind(config: Config) -> Result<Self> {
        let lease = config.running_lease()?;
        let control = TcpListener::bind((config.address.as_str(), config.control_port)).await?;
        let bridge = TcpListener::bind((config.address.as_str(), config.bridge_port)).await?;
        let store = Store::open(config.workflows_dir())?;
        let shared = Shared {
            config,
            state: Mutex::new(State::default()),
            store,
            shutdown: CancellationToken::new(),
            show_window: Arc::new(Notify::new()),
        };
        Ok(Self {
            handle: BackendHandle(Arc::new(shared)),
            control,
            bridge,
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
                    tasks.spawn(async move { if let Err(e) = control::connection(backend, socket).await { eprintln!("control connection: {e}"); } });
                }
                accepted = self.bridge.accept() => {
                    let (socket, _) = accepted?; let backend = self.handle.clone();
                    tasks.spawn(async move { if let Err(e) = bridge::connection(backend, socket).await { eprintln!("bridge connection: {e}"); } });
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
