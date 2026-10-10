//! Resolves the host, injects the Bridge, and waits for its backend registration.

use super::{Application, Result, blocking};
use flint_contracts::config::StateDir;
use flint_contracts::host_settings::HostSettings;
use flint_contracts::protocol::{Failure, FailureCode};
use flint_hosts::{Attach, HostKind};
use std::time::{Duration, Instant};

#[derive(Debug, serde::Serialize, specta::Type)]
pub struct AttachResult {
    pub pid: u32,
    pub host: HostKind,
    pub instance_id: String,
    pub execution_ready: bool,
}

/// Reject unsupported hosts before the caller starts a backend or prepares files.
fn resolve(pid: u32, host: Option<HostKind>) -> Result<(HostKind, Attach)> {
    let host = match host {
        Some(host) => host,
        None => {
            flint_hosts::candidate(pid)
                .ok_or_else(|| {
                    Failure::with_message(
                        FailureCode::InvalidArguments,
                        format!("process {pid} is not a recognized host; specify its host kind"),
                    )
                })?
                .host
        }
    };
    Ok((
        host,
        flint_hosts::attachment(host).map_err(|error| Failure::caused_by(FailureCode::InvalidArguments, &error))?,
    ))
}

/// Ends a wait for the injected Bridge to register once its side reports why it
/// could not start, or once `deadline` passes.
fn pending(state: &StateDir, pid: u32, deadline: Instant) -> Result<()> {
    if let Some(reason) = flint_hosts::attach_error(state, pid) {
        return Err(Failure::with_message(FailureCode::AttachFailed, reason));
    }
    if Instant::now() >= deadline {
        return Err(Failure::new(FailureCode::AttachTimeout));
    }
    Ok(())
}

impl Application {
    pub async fn attach(&self, pid: u32, host: Option<HostKind>, name: Option<String>) -> Result<AttachResult> {
        let (host, attach) = blocking(move || resolve(pid, host)).await?;
        let name = name.unwrap_or_else(|| host.to_string());
        self.start_backend().await?;
        let settings = HostSettings {
            address: self.config.endpoints.bridge.ip().to_string(),
            port: self.config.endpoints.bridge.port(),
            name: name.clone(),
        };
        let state = self.config.state.clone();
        blocking(move || {
            attach.inject(pid, &state, settings).map_err(|error| {
                Failure::caused_by(
                    FailureCode::AttachFailed,
                    error.context("cannot inject the Bridge").as_ref(),
                )
            })
        })
        .await?;
        // Matching the name skips the instance a re-pointed Bridge is replacing.
        let deadline = Instant::now() + self.config.timeout;
        loop {
            if let Some(instance) = self
                .instances(None)
                .await?
                .into_iter()
                .find(|instance| instance.pid == pid && instance.instance_name == name)
            {
                return Ok(AttachResult {
                    pid,
                    host,
                    instance_id: instance.instance_id,
                    execution_ready: instance.execution_ready,
                });
            }
            pending(&self.config.state, pid, deadline)?;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
