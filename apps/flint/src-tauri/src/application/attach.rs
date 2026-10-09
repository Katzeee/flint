//! Resolves the host, injects the Bridge, and waits for its backend registration.

use super::{blocking, Application, Result};
use flint_backend::config::Config;
use flint_contracts::protocol::{Failure, FailureCode};
use flint_hosts::{Attach, AttachRequest, HostKind};
use std::time::{Duration, Instant};

#[derive(Debug, serde::Serialize)]
pub struct AttachResult {
    pub pid: u32,
    pub host: HostKind,
    pub instance_id: String,
    pub execution_ready: bool,
}

struct ResolvedAttach {
    host: HostKind,
    pid: u32,
    attach: Attach,
}

/// Reject unsupported hosts before the caller starts a backend or prepares files.
fn resolve(pid: u32, host: Option<HostKind>) -> Result<ResolvedAttach> {
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
    Ok(ResolvedAttach {
        host,
        pid,
        attach: flint_hosts::attachment(host)
            .map_err(|error| Failure::caused_by(FailureCode::InvalidArguments, &error))?,
    })
}

impl ResolvedAttach {
    /// Inject the Bridge into the host process, connecting to `config`'s Bridge endpoint.
    fn inject(self, config: &Config, name: &str) -> Result<()> {
        let request = AttachRequest {
            address: config.address.clone(),
            port: config.bridge_port,
            name: name.to_string(),
        };
        self.attach.inject(self.pid, &request).map_err(|error| {
            Failure::caused_by(
                FailureCode::AttachFailed,
                error.context("cannot inject the Bridge").as_ref(),
            )
        })
    }
}

/// Ends a wait for the injected Bridge to register once its side reports why it
/// could not start, or once `deadline` passes.
fn pending(pid: u32, deadline: Instant) -> Result<()> {
    if let Some(reason) = flint_hosts::attach_error(pid) {
        return Err(Failure::with_message(FailureCode::AttachFailed, reason));
    }
    if Instant::now() >= deadline {
        return Err(Failure::new(FailureCode::AttachTimeout));
    }
    Ok(())
}

impl Application {
    pub async fn attach(
        &self,
        pid: u32,
        host: Option<HostKind>,
        name: Option<String>,
    ) -> Result<AttachResult> {
        let resolved = blocking(move || resolve(pid, host)).await?;
        let host = resolved.host;
        let name = name.unwrap_or_else(|| host.to_string());
        self.start_backend().await?;
        let config = self.config.clone();
        let injected = name.clone();
        blocking(move || resolved.inject(&config, &injected)).await?;
        // Matching the name skips the instance a re-pointed Bridge is replacing.
        let deadline = Instant::now() + Duration::from_secs_f64(self.config.timeout);
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
            pending(pid, deadline)?;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
