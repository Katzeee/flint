//! Backend lifecycle, observation, and control requests.
use super::{Application, Result as ApplicationResult};
use flint_backend::config::{lock_contended, Config};
use flint_contracts::protocol::{envelope::Payload, *};
use fs2::FileExt;
use futures_util::SinkExt;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{net::TcpStream, time::Instant};
use uuid::Uuid;

/// Lifecycle operations handle I/O conditions before they become product failures.
#[derive(Debug, thiserror::Error)]
enum ControlError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Failure(#[from] Failure),
}

impl From<ControlError> for Failure {
    fn from(error: ControlError) -> Self {
        match error {
            ControlError::Failure(failure) => failure,
            ControlError::Io(error) => Failure::caused_by(FailureCode::BackendUnavailable, &error),
        }
    }
}

type Result<T> = std::result::Result<T, ControlError>;

fn protocol(message: &'static str) -> ControlError {
    Failure::with_message(FailureCode::InternalError, message).into()
}

async fn request(config: &Config, payload: Payload) -> Result<Payload> {
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
    let exchange = async {
        let socket = TcpStream::connect(endpoint).await?;
        let mut wire = framed(socket);
        let id = Uuid::new_v4().simple().to_string();
        wire.send(envelope(id.clone(), payload)).await?;
        let response = read_envelope(&mut wire).await.map_err(|error| {
            if error.kind() == io::ErrorKind::InvalidData {
                Failure::caused_by(FailureCode::InternalError, &error).into()
            } else {
                ControlError::Io(error)
            }
        })?;
        if response.request_id != id {
            return Err(protocol("response request identity mismatch"));
        }
        match response.payload.unwrap() {
            Payload::Failure(failure) => Err(failure.into()),
            payload => Ok(payload),
        }
    };
    tokio::time::timeout_at(deadline, exchange)
        .await
        .map_err(|_| {
            Failure::with_message(FailureCode::BackendUnavailable, "backend request timed out")
        })?
}
#[derive(Debug, serde::Serialize)]
pub struct BackendStopped {
    pub stopped_pid: Option<u32>,
}

impl Application {
    /// Observes the backend without starting it or creating runtime files.
    pub async fn backend_status(&self) -> ApplicationResult<Option<PingResponse>> {
        Ok(self
            .probe(Instant::now() + Duration::from_secs_f64(self.config.timeout))
            .await?)
    }
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
            Ok(_) => Err(protocol("unexpected ping response")),
            Err(ControlError::Io(error)) if error.kind() == io::ErrorKind::ConnectionRefused => {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
    pub async fn start_backend(&self) -> ApplicationResult<PingResponse> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
        let _lock = self.lock(deadline).await?;
        Ok(self.ensure_locked(deadline).await?)
    }
    async fn ensure_locked(&self, deadline: Instant) -> Result<PingResponse> {
        let initial = self.probe(deadline).await?;
        if let Some(ping) = initial.as_ref().filter(|p| p.ready) {
            return Ok(ping.clone());
        }
        let mut child = None;
        if initial.is_none() && self.lease_available()? {
            child = Some(self.spawn()?);
        }
        loop {
            if let Some(ping) = self.probe(deadline).await?.filter(|p| p.ready) {
                return Ok(ping);
            }
            if let Some(child) = child.as_mut() {
                if child.try_wait()?.is_some() {
                    return Err(Failure::with_message(
                        FailureCode::BackendUnavailable,
                        "backend startup failed; see backend.log",
                    )
                    .into());
                }
            }
            pause(deadline).await?;
        }
    }
    fn spawn(&self) -> io::Result<std::process::Child> {
        let mut command = Command::new(std::env::current_exe()?);
        command.arg("serve");
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
        command.spawn()
    }
    pub async fn stop_backend(&self) -> ApplicationResult<BackendStopped> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
        let _lock = self.lock(deadline).await?;
        Ok(self.stop_locked(deadline).await?)
    }
    async fn stop_locked(&self, deadline: Instant) -> Result<BackendStopped> {
        let status = loop {
            if let Some(status) = self.probe(deadline).await? {
                break status;
            }
            if self.lease_available()? {
                return Ok(BackendStopped { stopped_pid: None });
            }
            pause(deadline).await?;
        };
        let response = request_until(
            (self.config.address.as_str(), self.config.control_port),
            Payload::StopBackendRequest(StopBackendRequest {}),
            deadline,
        )
        .await?;
        if !matches!(response, Payload::StopBackendResponse(_)) {
            return Err(protocol("invalid shutdown acknowledgement"));
        }
        loop {
            match self.probe(deadline).await {
                Ok(None) if self.lease_available()? => {
                    return Ok(BackendStopped {
                        stopped_pid: Some(status.pid),
                    })
                }
                Err(ControlError::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset | io::ErrorKind::UnexpectedEof
                    ) => {}
                Err(e) => return Err(e),
                _ => {}
            }
            pause(deadline).await?;
        }
    }
    pub async fn restart_backend(&self) -> ApplicationResult<PingResponse> {
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
        let _lock = self.lock(deadline).await?;
        self.stop_locked(deadline).await?;
        Ok(self.ensure_locked(deadline).await?)
    }
}
async fn pause(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(Failure::with_message(
            FailureCode::BackendUnavailable,
            "backend lifecycle timed out",
        )
        .into());
    }
    tokio::time::sleep_until((Instant::now() + Duration::from_millis(50)).min(deadline)).await;
    Ok(())
}

#[derive(Serialize)]
pub struct Snapshot {
    /// No backend is listening. This is distinct from a running backend with no instances.
    pub backend: Option<PingResponse>,
    pub instances: Vec<InstanceInfo>,
}

impl Application {
    /// Runs the backend service in this process, independently of a desktop window.
    pub async fn serve(&self) -> ApplicationResult<()> {
        let backend = flint_backend::Backend::bind(self.config.clone())
            .await
            .map_err(Failure::from)?;
        let stop = backend.handle();
        let interrupt = tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            let _ = stop.request_stop();
        });
        let result = backend
            .run()
            .await
            .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, error.as_ref()));
        interrupt.abort();
        result
    }

    pub async fn snapshot(&self) -> ApplicationResult<Snapshot> {
        let stopped = Snapshot {
            backend: None,
            instances: vec![],
        };
        let Some(backend) = self.backend_status().await? else {
            return Ok(stopped);
        };
        match self.instances(None).await {
            Ok(instances) => Ok(Snapshot {
                backend: Some(backend),
                instances,
            }),
            // The backend can stop between the two requests.
            Err(failure)
                if failure.is(FailureCode::BackendUnavailable)
                    && self.backend_status().await?.is_none() =>
            {
                Ok(stopped)
            }
            Err(failure) => Err(failure),
        }
    }

    pub async fn instances(
        &self,
        instance_type: Option<String>,
    ) -> ApplicationResult<Vec<InstanceInfo>> {
        self.query(
            Payload::ListInstancesRequest(ListInstancesRequest { instance_type }),
            |response| match response {
                Payload::ListInstancesResponse(response) => Some(response.instances),
                _ => None,
            },
        )
        .await
    }

    pub(super) async fn query<T>(
        &self,
        payload: Payload,
        response: fn(Payload) -> Option<T>,
    ) -> ApplicationResult<T> {
        let payload = request(&self.config, payload).await?;
        response(payload).ok_or_else(|| {
            Failure::with_message(FailureCode::InternalError, "invalid backend response")
        })
    }
}
