//! Startup instructions shared by the injector
//! and the injected bootstrap. Plans name runtime entries, never hosts.

use crate::host_settings::HostSettings;
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Prepared by a platform from its host's entry and consumed by the injected bootstrap.
#[derive(Deserialize, Serialize)]
#[serde(tag = "runtime", rename_all = "snake_case")]
pub enum RuntimePlan {
    Cpython(CpythonPlan),
    Mono(MonoPlan),
}

/// The Python library and host module whose manager starts the Bridge.
#[derive(Deserialize, Serialize)]
pub struct CpythonPlan {
    pub import_root: PathBuf,
    pub module: String,
    pub settings: HostSettings,
}

/// The managed assembly and host entry used by the .NET attach adapter.
#[derive(Deserialize, Serialize)]
pub struct MonoPlan {
    pub assembly: PathBuf,
    /// The scripting domain in which the entry must execute.
    pub domain: String,
    pub entry: String,
    /// Re-enter Restore after a new domain loads these dependencies; None disables it.
    /// The host entry decides when and whether its Bridge can resume.
    pub reload_assemblies: Option<Vec<String>>,
    pub native_library: PathBuf,
    pub settings: HostSettings,
}

/// NUL-terminated JSON passed to the bootstrap's remote entry point.
/// The injector supplies the plan and resolves the report path.
#[derive(Deserialize, Serialize)]
pub struct BootstrapRequest {
    pub plan: RuntimePlan,
    /// Written once the injected side knows the outcome: empty when the Bridge connected
    /// with the plan's settings, otherwise why it did not. Replaced whole, never written in place.
    pub report_path: PathBuf,
}

pub fn write_report(path: &Path, outcome: Result<(), &str>) -> io::Result<()> {
    let staged = path.with_extension("staged");
    fs::write(&staged, outcome.err().unwrap_or_default())?;
    fs::rename(staged, path)
}

/// None while the injected side is still working.
pub fn read_report(path: &Path) -> Option<Result<(), String>> {
    let report = fs::read_to_string(path).ok()?;
    Some(if report.is_empty() { Ok(()) } else { Err(report) })
}
