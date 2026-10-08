use anyhow::{Context, Result};
use flint_config::{lock_contended, Config};
use flint_contracts::protocol::{envelope::Payload, *};
use fs2::FileExt;
use futures_util::SinkExt;
use std::{
    fs::{File, OpenOptions},
    io,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{net::TcpStream, time::Instant};
use uuid::Uuid;

/// A failure the backend answers with propagates as the `Failure` itself.
pub async fn request(config: &Config, payload: Payload) -> Result<Payload> {
    request_until(
        (config.address.as_str(), config.control_port),
        payload,
        Instant::now() + Duration::from_secs_f64(config.timeout),
    )
    .await
}
async fn request_until(
    endpoint: (&str, u16),
    payload: Payload,
    deadline: Instant,
) -> Result<Payload> {
    tokio::time::timeout_at(deadline, async {
        let socket = TcpStream::connect(endpoint).await?;
        let mut wire = framed(socket);
        let id = Uuid::new_v4().simple().to_string();
        wire.send(envelope(id.clone(), payload)).await?;
        let response = read_envelope(&mut wire)
            .await
            .context("backend closed before replying")?;
        anyhow::ensure!(
            response.request_id == id,
            "response request identity mismatch"
        );
        match response.payload.context("missing response payload")? {
            Payload::Failure(failure) => Err(failure.into()),
            payload => Ok(payload),
        }
    })
    .await
    .context("backend request timed out")?
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
        Ok(self.config.running_lease()?.is_some())
    }
    async fn probe(&self, deadline: Instant) -> Result<Option<PingResponse>> {
        match request_until(
            (self.config.address.as_str(), self.config.control_port),
            Payload::PingRequest(PingRequest {}),
            deadline,
        )
        .await
        {
            Ok(Payload::PingResponse(ping)) => Ok(Some(ping)),
            Ok(_) => anyhow::bail!("unexpected ping response"),
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
            let mut command = Command::new(std::env::current_exe()?);
            command.arg("serve");
            if self.no_tray {
                command.arg("--no-tray");
            }
            command.stdin(Stdio::null());
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
                    "backend startup failed; see backend.log"
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
            (self.config.address.as_str(), self.config.control_port),
            Payload::StopBackendRequest(StopBackendRequest {}),
            deadline,
        )
        .await?;
        anyhow::ensure!(
            matches!(
                response,
                Payload::StopBackendResponse(StopBackendResponse { stopping: true })
            ),
            "invalid shutdown acknowledgement"
        );
        loop {
            match self.probe(deadline).await {
                Ok(None) if self.lease_available()? => {
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
    anyhow::ensure!(Instant::now() < deadline, "backend lifecycle timed out");
    tokio::time::sleep_until((Instant::now() + Duration::from_millis(50)).min(deadline)).await;
    Ok(())
}
pub fn status_json(p: PingResponse) -> serde_json::Value {
    serde_json::json!({"ready": p.ready, "pid": p.pid, "bridge_address": p.bridge_address, "bridge_port": p.bridge_port})
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
                out["error"] = serde_json::to_value(e)?;
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
        _ => anyhow::bail!("unexpected response"),
    })
}
