use anyhow::{Context, Result};
use flint_core::Config;
use flint_protocol::{envelope::Payload, *};
use fs2::FileExt;
use futures_util::{SinkExt, StreamExt};
use std::{
    fs::{File, OpenOptions},
    io,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{net::TcpStream, time::Instant};
use tokio_util::codec::Framed;
use uuid::Uuid;

#[derive(Debug)]
pub struct RemoteError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RemoteError {}

pub async fn request(config: &Config, payload: Payload) -> Result<Payload> {
    request_until(
        config,
        payload,
        Instant::now() + Duration::from_secs_f64(config.timeout),
    )
    .await
}
async fn request_until(config: &Config, payload: Payload, deadline: Instant) -> Result<Payload> {
    tokio::time::timeout_at(deadline, async {
        let socket = TcpStream::connect((config.host.as_str(), config.port)).await?;
        let mut wire = Framed::new(socket, EnvelopeCodec::default());
        let id = Uuid::new_v4().simple().to_string();
        wire.send(Envelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: id.clone(),
            payload: Some(payload),
        })
        .await?;
        let response = wire.next().await.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Backend closed before replying",
            )
        })??;
        anyhow::ensure!(
            response.request_id == id,
            "Response request identity mismatch"
        );
        let payload = response.payload.context("Missing response payload")?;
        if let Payload::ProtocolError(error) = payload {
            let code = ErrorCode::try_from(error.code)
                .map(|e| {
                    e.as_str_name()
                        .trim_start_matches("ERROR_CODE_")
                        .to_lowercase()
                })
                .unwrap_or_else(|_| "protocol_error".into());
            return Err(RemoteError {
                code,
                message: error.message,
            }
            .into());
        }
        Ok(payload)
    })
    .await
    .context("Backend request timed out")?
}
pub struct Lifecycle {
    pub config: Config,
    pub no_tray: bool,
}
impl Lifecycle {
    async fn lock(&self, deadline: Instant) -> Result<File> {
        let file = self.config.lock_file("lifecycle")?;
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => return Ok(file),
                Err(e) if lock_contended(&e) => pause(deadline).await?,
                Err(e) => return Err(e.into()),
            }
        }
    }
    fn lease_available(&self) -> Result<bool> {
        let file = self.config.lock_file("running")?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(true),
            Err(e) if lock_contended(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
    async fn probe(&self, deadline: Instant) -> Result<Option<PingResponse>> {
        match request_until(&self.config, Payload::PingRequest(PingRequest {}), deadline).await {
            Ok(Payload::PingResponse(ping)) => Ok(Some(ping)),
            Ok(_) => anyhow::bail!("Unexpected ping response"),
            Err(e)
                if e.downcast_ref::<io::Error>()
                    .map_or(false, |e| e.kind() == io::ErrorKind::ConnectionRefused) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
    pub async fn ensure(&self) -> Result<PingResponse> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
        let _lock = self.lock(deadline).await?;
        self.ensure_locked(deadline).await
    }
    async fn ensure_locked(&self, deadline: Instant) -> Result<PingResponse> {
        let initial = self.probe(deadline).await?;
        if let Some(ping) = initial.as_ref().filter(|p| p.ready) {
            return Ok(ping.clone());
        }
        let mut child = None;
        if initial.is_none() && self.lease_available()? {
            anyhow::ensure!(
                ["localhost", "127.0.0.1", "::1"].contains(&self.config.host.as_str()),
                "Automatic startup requires a local endpoint"
            );
            let mut command = Command::new(std::env::current_exe()?);
            command.args([
                "serve",
                "--host",
                &self.config.host,
                "--port",
                &self.config.port.to_string(),
                "--registry-host",
                &self.config.registry_host,
                "--registry-port",
                &self.config.registry_port.to_string(),
            ]);
            if self.no_tray {
                command.arg("--no-tray");
            }
            command
                .env("FLINT_STATE_DIR", &self.config.state_dir)
                .stdin(Stdio::null());
            let log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.config.runtime_dir().join("backend.log"))?;
            command.stdout(log.try_clone()?).stderr(log);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // The detached service must not retain the CLI's redirected pipes.
                // Otherwise a shell waiting for EOF can wait for the backend's lifetime.
                unsafe {
                    #[link(name = "kernel32")]
                    extern "system" {
                        fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
                        fn SetHandleInformation(
                            handle: *mut std::ffi::c_void,
                            mask: u32,
                            flags: u32,
                        ) -> i32;
                    }
                    for kind in [-10i32, -11, -12] {
                        SetHandleInformation(GetStdHandle(kind as u32), 1, 0);
                    }
                }
                command.creation_flags(0x08000000 | 0x00000200);
            }
            child = Some(command.spawn()?);
        }
        loop {
            if let Some(ping) = self.probe(deadline).await?.filter(|p| p.ready) {
                return Ok(ping);
            }
            if let Some(child) = child.as_mut() {
                anyhow::ensure!(
                    child.try_wait()?.is_none(),
                    "Backend startup failed; see backend.log"
                );
            }
            pause(deadline).await?;
        }
    }
    pub async fn stop(&self) -> Result<serde_json::Value> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
        let _lock = self.lock(deadline).await?;
        self.stop_locked(deadline).await
    }
    async fn stop_locked(&self, deadline: Instant) -> Result<serde_json::Value> {
        let status = loop {
            if let Some(status) = self.probe(deadline).await? {
                break status;
            }
            if self.lease_available()? {
                return Ok(serde_json::json!({"stopped": true, "already_stopped": true}));
            }
            pause(deadline).await?;
        };
        let response = request_until(
            &self.config,
            Payload::StopBackendRequest(StopBackendRequest {
                backend_id: status.backend_id.clone(),
            }),
            deadline,
        )
        .await?;
        anyhow::ensure!(
            matches!(
                response,
                Payload::StopBackendResponse(StopBackendResponse { stopping: true })
            ),
            "Invalid shutdown acknowledgement"
        );
        loop {
            match self.probe(deadline).await {
                Ok(None) if self.lease_available()? => {
                    return Ok(serde_json::json!({"stopped": true, "pid": status.pid}))
                }
                Ok(Some(p)) if p.backend_id != status.backend_id => {
                    return Ok(serde_json::json!({"stopped": true, "pid": status.pid}))
                }
                Err(e)
                    if e.downcast_ref::<io::Error>().map_or(false, |e| {
                        matches!(
                            e.kind(),
                            io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof
                        )
                    }) => {}
                Err(e) => return Err(e),
                _ => {}
            }
            pause(deadline).await?;
        }
    }
    pub async fn restart(&self) -> Result<PingResponse> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
        let _lock = self.lock(deadline).await?;
        self.stop_locked(deadline).await?;
        self.ensure_locked(deadline).await
    }
}
async fn pause(deadline: Instant) -> Result<()> {
    anyhow::ensure!(Instant::now() < deadline, "Backend lifecycle timed out");
    tokio::time::sleep_until((Instant::now() + Duration::from_millis(50)).min(deadline)).await;
    Ok(())
}
fn lock_contended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock || cfg!(windows) && error.raw_os_error() == Some(33)
}

pub fn status_json(p: PingResponse) -> serde_json::Value {
    serde_json::json!({"ready": p.ready, "pid": p.pid, "backend_id": p.backend_id, "registry_host": p.registry_host, "registry_port": p.registry_port})
}
pub fn instance_json(i: InstanceInfo) -> serde_json::Value {
    serde_json::json!({"instance_id": i.instance_id, "instance_name": i.instance_name, "instance_type": i.instance_type,
        "pid": i.pid, "runtime_version": i.runtime_version, "bridge_version": i.bridge_version, "execution_ready": i.execution_ready})
}
pub fn payload_json(p: Payload) -> Result<serde_json::Value> {
    use serde_json::json;
    let status = |n| match ExecutionStatus::try_from(n) {
        Ok(ExecutionStatus::Pending) => "pending",
        Ok(ExecutionStatus::Running) => "running",
        Ok(ExecutionStatus::Succeeded) => "succeeded",
        Ok(ExecutionStatus::Failed) => "failed",
        _ => "unknown",
    };
    Ok(match p {
        Payload::PingResponse(p) => status_json(p),
        Payload::ListInstancesResponse(p) => {
            json!({"instances": p.instances.into_iter().map(instance_json).collect::<Vec<_>>()})
        }
        Payload::StartWorkflowResponse(p) => json!({"workflow_id": p.workflow_id}),
        Payload::ExecutionResult(p) => {
            let mut out = json!({"execution_id": p.execution_id, "status": status(p.status)});
            if let Some(t) = p.traceback {
                out["traceback"] = t.into();
            }
            if let Some(e) = p.error {
                out["error"] = e.into();
            }
            out
        }
        Payload::GetExecutionResponse(p) => {
            let mut out = json!({"execution_id": p.execution_id, "workflow_id": p.workflow_id, "instance_id": p.instance_id,
                "name": p.name, "status": status(p.status), "stdout": p.stdout, "stderr": p.stderr,
                "started_at": p.started_at, "finished_at": p.finished_at, "updated_at": p.updated_at,
                "traceback": p.traceback, "error": p.error});
            if let Some(code) = p.code {
                out["code"] = code.into();
            }
            out
        }
        Payload::ShowWindowResponse(p) => json!({"accepted": p.accepted}),
        _ => anyhow::bail!("Unexpected response"),
    })
}
