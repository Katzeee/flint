//! Attach orchestration: stage the embedded bootstrap and Bridge package, then
//! ask `flint-hosts` to inject them into a host process. The CLI confirms the
//! Bridge registered by polling the backend.

use anyhow::{bail, ensure, Result};
use flint_config::Config;
use flint_contracts::host::HostKind;
use flint_hosts::{AttachRequest, Runtime};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

// Produced by build.rs and embedded. The bootstrap is injected; CPython hosts
// load the Python package, and Unity loads its managed adapter plus the
// native core. All are empty on non-Windows targets, where attach is disabled.
const BOOTSTRAP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint-bootstrap.dll"));
const PYTHON_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint-python.zip"));
const UNITY_BRIDGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint-unity.dll"));
const CORE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint_bridge_core.dll"));

/// Extract an embedded asset to a content-addressed temporary path, reusing an
/// identical copy. Mirrors how the Python Bridge extracts its native core.
/// Content addressing means new bytes never overwrite a file a previous attach
/// may still have loaded (which Windows would refuse).
fn stage(name: &str, bytes: &[u8]) -> Result<PathBuf> {
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

/// Reject known integrations without an attach implementation before preparing files.
pub fn validate_host(host: HostKind) -> Result<()> {
    match host {
        HostKind::Maya
        | HostKind::Max
        | HostKind::Blender
        | HostKind::Unity
        | HostKind::StandalonePython => Ok(()),
        HostKind::StandaloneCsharp => bail!("Attach is not implemented for host kind: {host}"),
    }
}

/// Inject the Bridge into host process `pid`, connecting to `config`'s Bridge endpoint.
pub fn inject(config: &Config, pid: u32, host: HostKind, name: &str) -> Result<()> {
    validate_host(host)?;
    ensure!(
        !BOOTSTRAP.is_empty(),
        "This build has no attach bootstrap; attach is only available on Windows"
    );
    let bootstrap = stage("flint-bootstrap.dll", BOOTSTRAP)?;
    let (runtime, payload, core) = match host {
        HostKind::Maya | HostKind::Max | HostKind::Blender | HostKind::StandalonePython => (
            Runtime::Cpython,
            stage("flint-python.zip", PYTHON_ZIP)?,
            None,
        ),
        HostKind::Unity => (
            Runtime::Dotnet,
            stage("Flint.Unity.dll", UNITY_BRIDGE)?,
            Some(stage("flint_bridge_core.dll", CORE)?),
        ),
        HostKind::StandaloneCsharp => bail!("Attach is not implemented for host kind: {host}"),
    };
    let request = AttachRequest {
        host,
        runtime,
        bootstrap,
        payload,
        core,
        address: config.address.clone(),
        port: config.bridge_port,
        name: name.to_string(),
    };
    flint_hosts::attach(pid, &request)
}
