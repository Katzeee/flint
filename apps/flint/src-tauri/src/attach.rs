//! Attach orchestration: ask `flint-hosts` to stage the host's attach layout
//! and inject the bootstrap. Callers confirm the Bridge registered by polling
//! the backend.

use flint_backend::config::Config;
use flint_contracts::protocol::{Failure, FailureCode};
use flint_hosts::{AttachRequest, HostKind};
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error("process {0} is not a recognized host; specify its host kind")]
    UnknownHost(u32),
    #[error(transparent)]
    Unsupported(#[from] flint_hosts::Unsupported),
    #[error("cannot inject the Bridge")]
    Injection(#[source] anyhow::Error),
    /// The injected side's own report of why its Bridge could not start.
    #[error("{0}")]
    Refused(String),
    #[error("the Bridge did not register before the timeout")]
    Timeout,
}

impl From<AttachError> for Failure {
    fn from(error: AttachError) -> Self {
        let code = match error {
            AttachError::UnknownHost(_) | AttachError::Unsupported(_) => {
                FailureCode::InvalidArguments
            }
            AttachError::Injection(_) | AttachError::Refused(_) => FailureCode::AttachFailed,
            AttachError::Timeout => FailureCode::AttachTimeout,
        };
        Failure::caused_by(code, &error)
    }
}

/// Resolves the host kind of `pid`, rejecting integrations without an attach
/// implementation before any files are prepared.
pub fn resolve(pid: u32, host: Option<HostKind>) -> Result<HostKind, AttachError> {
    let host = match host {
        Some(host) => host,
        None => {
            flint_hosts::candidate(pid)
                .ok_or(AttachError::UnknownHost(pid))?
                .host
        }
    };
    flint_hosts::attachable(host)?;
    Ok(host)
}

/// Inject the Bridge into host process `pid`, connecting to `config`'s Bridge endpoint.
pub fn inject(config: &Config, pid: u32, host: HostKind, name: &str) -> Result<(), AttachError> {
    let request = AttachRequest {
        host,
        address: config.address.clone(),
        port: config.bridge_port,
        name: name.to_string(),
    };
    flint_hosts::attach(pid, &request).map_err(AttachError::Injection)
}

/// Ends a wait for the injected Bridge to register once its side reports why it
/// could not start, or once `deadline` passes.
pub fn pending(pid: u32, deadline: Instant) -> Result<(), AttachError> {
    if let Some(reason) = flint_hosts::attach_error(pid) {
        return Err(AttachError::Refused(reason));
    }
    if Instant::now() >= deadline {
        return Err(AttachError::Timeout);
    }
    Ok(())
}
