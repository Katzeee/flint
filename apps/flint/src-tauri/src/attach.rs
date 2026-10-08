//! Attach orchestration: stage the embedded bootstrap and Bridge package, then
//! ask `flint-hosts` to inject them into a host process. Callers confirm the
//! Bridge registered by polling the backend.

use anyhow::{ensure, Context, Result};
use flint_config::Config;
use flint_contracts::host::HostKind;
use flint_contracts::protocol::{Failure, FailureCode};
use flint_hosts::{AttachRequest, Runtime};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Instant;

/// An embedded file and the name it is staged under.
type Asset = (&'static str, &'static [u8]);

// Produced by build.rs and embedded. The bootstrap is injected; CPython hosts
// load the Python package, and Unity loads its managed adapter plus the
// native core. All are empty on non-Windows targets, where attach is disabled.
const BOOTSTRAP: Asset = (
    "flint-bootstrap.dll",
    include_bytes!(concat!(env!("OUT_DIR"), "/flint-bootstrap.dll")),
);
const PYTHON_ZIP: Asset = (
    "flint-python.zip",
    include_bytes!(concat!(env!("OUT_DIR"), "/flint-python.zip")),
);
const UNITY_BRIDGE: Asset = (
    "Flint.Unity.dll",
    include_bytes!(concat!(env!("OUT_DIR"), "/flint-unity.dll")),
);
const CORE: Asset = (
    "flint_bridge_core.dll",
    include_bytes!(concat!(env!("OUT_DIR"), "/flint_bridge_core.dll")),
);

#[derive(Debug, thiserror::Error)]
pub enum AttachError {
    #[error("process {0} is not a recognized host; specify its host kind")]
    UnknownHost(u32),
    #[error("attach is not implemented for host kind {0}")]
    Unsupported(HostKind),
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

/// Extract an embedded asset to a content-addressed temporary path, reusing an
/// identical copy. Mirrors how the Python Bridge extracts its native core.
/// Content addressing means new bytes never overwrite a file a previous attach
/// may still have loaded (which Windows would refuse).
fn stage(asset: Asset) -> Result<PathBuf> {
    store(asset).with_context(|| format!("cannot stage {}", asset.0))
}

fn store((name, bytes): Asset) -> Result<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let directory = std::env::temp_dir()
        .join("flint")
        .join("attach")
        .join(format!("{:016x}", hasher.finish()));
    std::fs::create_dir_all(&directory)?;
    let target = directory.join(name);
    if target.exists() {
        return Ok(target);
    }
    let temporary = directory.join(format!("{name}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)?;
    // A concurrent attach may have created it first; either way it is our bytes.
    if std::fs::rename(&temporary, &target).is_err() && !target.exists() {
        std::fs::rename(&temporary, &target)?;
    }
    let _ = std::fs::remove_file(&temporary);
    Ok(target)
}

/// Resolves the host kind of `pid`, rejecting integrations without an attach
/// implementation before any files are prepared.
pub fn resolve(pid: u32, host: Option<HostKind>) -> Result<HostKind, AttachError> {
    let host = match host {
        Some(host) => host,
        None => flint_hosts::candidate(pid).ok_or(AttachError::UnknownHost(pid))?.host,
    };
    match host {
        HostKind::StandaloneCsharp => Err(AttachError::Unsupported(host)),
        _ => Ok(host),
    }
}

/// Inject the Bridge into host process `pid`, connecting to `config`'s Bridge endpoint.
pub fn inject(config: &Config, pid: u32, host: HostKind, name: &str) -> Result<(), AttachError> {
    let (runtime, payload, core) = match host {
        HostKind::Maya | HostKind::Max | HostKind::Blender | HostKind::StandalonePython => {
            (Runtime::Cpython, PYTHON_ZIP, None)
        }
        HostKind::Unity => (Runtime::Dotnet, UNITY_BRIDGE, Some(CORE)),
        HostKind::StandaloneCsharp => return Err(AttachError::Unsupported(host)),
    };
    let inject = || -> Result<()> {
        ensure!(
            !BOOTSTRAP.1.is_empty(),
            "this build has no attach bootstrap; attach is only available on Windows"
        );
        let request = AttachRequest {
            host,
            runtime,
            bootstrap: stage(BOOTSTRAP)?,
            payload: stage(payload)?,
            core: core.map(stage).transpose()?,
            address: config.address.clone(),
            port: config.bridge_port,
            name: name.to_string(),
        };
        flint_hosts::attach(pid, &request)
    };
    inject().map_err(AttachError::Injection)
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
