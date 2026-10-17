//! Resolves the host, injects the Bridge, and waits for its backend registration.

use super::{Application, Result, apps, apps::App, blocking};
use flint_contracts::config::StateDir;
use flint_contracts::host_settings::HostSettings;
use flint_contracts::protocol::{Failure, FailureCode};
use flint_hosts::{Attach, HostKind};
use std::time::{Duration, Instant};

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
    pub async fn attach(&self, pid: u32, host: Option<HostKind>, name: Option<String>) -> Result<App> {
        let (host, attach) = blocking(move || resolve(pid, host)).await?;
        let expected_id = blocking(move || {
            flint_contracts::instance::id_for_process(pid)
                .map_err(|error| Failure::caused_by(FailureCode::AttachFailed, error.as_ref()))
        })
        .await?;
        let name = name.unwrap_or_else(|| host.to_string());
        self.start_backend().await?;
        let settings = HostSettings {
            address: self.config.endpoints.bridge_target().ip().to_string(),
            port: self.config.endpoints.bridge_target().port(),
            name: name.clone(),
            token: String::new(),
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
                .find(|instance| instance.instance_id == expected_id && instance.instance_name == name)
            {
                return apps::of_instance(instance).await;
            }
            pending(&self.config.state, pid, deadline)?;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
