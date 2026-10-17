use crate::store::{Store, now};
use anyhow::{Context, Result};
use flint_contracts::config::Config;
use flint_contracts::lock::FileLock;
use flint_contracts::protocol::timing::HEARTBEAT_IDLE_TIMEOUT;
use flint_contracts::protocol::{envelope::Payload, *};
use futures_util::SinkExt;
use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

mod bridge;
mod control;
mod state;

use state::{Job, State};

const FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);

type Wire = flint_contracts::protocol::framing::Wire<TcpStream>;

#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("another backend owns this runtime")]
    Locked,
    #[error(transparent)]
    Startup(#[from] anyhow::Error),
}

impl From<BindError> for Failure {
    fn from(error: BindError) -> Self {
        match error {
            BindError::Locked => Failure::new(FailureCode::BackendLocked),
            BindError::Startup(error) => Failure::caused_by(FailureCode::InternalError, error.as_ref()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("executions are still active")]
pub struct BackendBusy;

impl From<BackendBusy> for Failure {
    fn from(_: BackendBusy) -> Self {
        Failure::new(FailureCode::BackendBusy)
    }
}

struct Shared {
    config: Config,
    state: Mutex<State>,
    store: Store,
    shutdown: CancellationToken,
}

#[derive(Clone)]
pub struct BackendHandle(Arc<Shared>);
impl BackendHandle {
    pub(crate) fn instances(&self) -> Vec<InstanceInfo> {
        let state = self.0.state.lock().unwrap();
        let mut result: Vec<_> = state
            .sessions
            .values()
            .map(|s| InstanceInfo {
                instance_id: s.registration.instance_id.clone(),
                instance_name: s.registration.instance_name.clone(),
                instance_type: s.registration.instance_type.clone(),
                pid: s.registration.pid,
                runtime_version: s.registration.runtime_version.clone(),
                bridge_version: s.registration.bridge_version.clone(),
                execution_ready: s.sender.is_some(),
            })
            .collect();
        result.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        result
    }
    pub(crate) fn status(&self) -> PingResponse {
        PingResponse {
            ready: !self.0.state.lock().unwrap().stopping,
            pid: std::process::id(),
            bridge_addresses: bridge_addresses(self.0.config.endpoints.bridge.ip()),
            bridge_port: self.0.config.endpoints.bridge.port().into(),
        }
    }
    pub fn request_stop(&self) -> Result<(), BackendBusy> {
        self.begin_stop()?;
        self.0.shutdown.cancel();
        Ok(())
    }
    /// Refuses new work; the caller ends the service once it has replied.
    fn begin_stop(&self) -> Result<(), BackendBusy> {
        let mut state = self.0.state.lock().unwrap();
        if !state.jobs.is_empty() {
            return Err(BackendBusy);
        }
        state.stopping = true;
        Ok(())
    }
}

fn bridge_addresses(listen: IpAddr) -> Vec<NetworkAddress> {
    if !listen.is_unspecified() {
        return vec![NetworkAddress {
            interface: String::new(),
            address: listen.to_string(),
        }];
    }
    let mut addresses: Vec<_> = sysinfo::Networks::new_with_refreshed_list()
        .iter()
        .flat_map(|(interface, data)| {
            data.ip_networks().iter().filter_map(move |network| match network.addr {
                IpAddr::V4(address) if !address.is_loopback() && !address.is_link_local() => Some(NetworkAddress {
                    interface: interface.clone(),
                    address: address.to_string(),
                }),
                _ => None,
            })
        })
        .collect();
    addresses.sort_by(|a, b| (&a.interface, &a.address).cmp(&(&b.interface, &b.address)));
    addresses
}

async fn listen(endpoint: SocketAddr) -> Result<TcpListener> {
    TcpListener::bind(endpoint)
        .await
        .with_context(|| format!("cannot listen on {endpoint}"))
}

pub struct Backend {
    handle: BackendHandle,
    control: TcpListener,
    bridge: TcpListener,
    _lease: FileLock,
}
impl Backend {
    pub async fn bind(config: Config) -> Result<Self, BindError> {
        let lease = config
            .state
            .runtime()
            .claim_backend()
            .context("cannot claim the backend runtime")?
            .ok_or(BindError::Locked)?;
        flint_contracts::config::bridge_token::load_or_create().context("cannot prepare the Bridge token")?;
        let control = listen(config.endpoints.control).await?;
        let bridge = listen(config.endpoints.bridge).await?;
        let store = Store::open(config.state.workflows()).context("cannot open the workflow store")?;
        let shared = Shared {
            config,
            state: Mutex::new(State::default()),
            store,
            shutdown: CancellationToken::new(),
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
                    tasks.spawn(async move { if let Err(e) = control::connection(backend, socket).await { eprintln!("control connection: {e:#}"); } });
                }
                accepted = self.bridge.accept() => {
                    let (socket, _) = accepted?; let backend = self.handle.clone();
                    tasks.spawn(async move { if let Err(e) = bridge::connection(backend, socket).await { eprintln!("bridge connection: {e:#}"); } });
                }
                _ = sweep.tick() => {
                    let expired: Vec<_> = self.handle.0.state.lock().unwrap().sessions.iter()
                        .filter(|(_,s)| s.heartbeat.elapsed() > HEARTBEAT_IDLE_TIMEOUT).map(|(id,_)| id.clone()).collect();
                    for id in expired { disconnect(&self.handle, &id); }
                }
                _ = tasks.join_next(), if !tasks.is_empty() => {}
            }
        }
        let ids: Vec<_> = self.handle.0.state.lock().unwrap().sessions.keys().cloned().collect();
        for id in ids {
            disconnect(&self.handle, &id);
        }
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        Ok(())
    }
}

fn finish_job(backend: &BackendHandle, job: Job, result: ExecutionResult) {
    let persisted = backend.0.store.update(&job.workflow, &job.execution, |entry| {
        entry.status = result.status();
        entry.finished_at = Some(now());
        entry.traceback = result.traceback.clone();
        entry.error = result.error.clone();
    });
    let result = match persisted {
        Ok(()) => result,
        Err(error) => ExecutionResult {
            execution_id: job.execution.clone(),
            status: ExecutionStatus::Failed as i32,
            traceback: None,
            error: Some(Failure::caused_by(FailureCode::ResultPersistenceFailed, &error)),
        },
    };
    let _ = job.result.send(result);
}

fn fail_disconnected(backend: &BackendHandle, jobs: Vec<Job>) {
    for job in jobs {
        let result = ExecutionResult {
            execution_id: job.execution.clone(),
            status: ExecutionStatus::Failed as i32,
            traceback: None,
            error: Some(Failure::new(FailureCode::ExecutionDisconnected)),
        };
        finish_job(backend, job, result);
    }
}

fn disconnect(backend: &BackendHandle, session_id: &str) {
    let jobs = backend.0.state.lock().unwrap().remove_session(session_id);
    fail_disconnected(backend, jobs);
}
