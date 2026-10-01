//! Attach orchestration: stage the embedded bootstrap and Bridge package, then
//! ask `flint-hosts` to inject them into a host process. The CLI confirms the
//! Bridge registered by polling the backend.

use anyhow::{bail, ensure, Result};
use flint_config::Config;
use flint_hosts::{AttachRequest, Runtime};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

// Produced by build.rs and embedded. The bootstrap is injected; CPython hosts
// load the Python package, and managed hosts load the attach assembly plus the
// native core. All are empty on non-Windows targets, where attach is disabled.
const BOOTSTRAP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint-bootstrap.dll"));
const PYTHON_ZIP: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint-python.zip"));
const UNITY_ATTACH: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/flint-unity-attach.dll"));
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

/// Inject the Bridge into host process `pid`, connecting to `config`'s registry.
pub fn inject(config: &Config, pid: u32, host: &str, name: Option<String>) -> Result<()> {
    ensure!(
        !BOOTSTRAP.is_empty(),
        "This build has no attach bootstrap; attach is only available on Windows"
    );
    let bootstrap = stage("flint-bootstrap.dll", BOOTSTRAP)?;
    let (runtime, payload, core) = match host {
        "maya" | "max" | "blender" | "python" => (
            Runtime::Cpython,
            stage("flint-python.zip", PYTHON_ZIP)?,
            None,
        ),
        "unity" => (
            Runtime::Dotnet,
            stage("Flint.Unity.Attach.dll", UNITY_ATTACH)?,
            Some(stage("flint_bridge_core.dll", CORE)?),
        ),
        other => bail!("Unknown host kind: {other}"),
    };
    let request = AttachRequest {
        host: host.to_string(),
        runtime,
        bootstrap,
        payload,
        core,
        address: config.registry_host.clone(),
        port: config.registry_port,
        name: name.unwrap_or_else(|| host.to_string()),
    };
    flint_hosts::attach(pid, &request)
}
