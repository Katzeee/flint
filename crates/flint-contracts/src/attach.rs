//! Startup instructions and file exchange conventions shared by the injector
//! and the injected bootstrap. Plans name runtime entries, never hosts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Prepared by a platform from its host's entry and consumed once by the injected bootstrap.
#[derive(Deserialize, Serialize)]
#[serde(tag = "runtime", rename_all = "snake_case")]
pub enum RuntimePlan {
    Cpython(CpythonPlan),
    Mono(MonoPlan),
}

/// Python source the bootstrap runs once under the GIL on its injected thread.
/// The source returns promptly and reports later failures through [`error_path`] itself.
#[derive(Deserialize, Serialize)]
pub struct CpythonPlan {
    pub source: String,
}

/// Invoke a static managed entry accepting one string and returning null on
/// success or a failure string. The bootstrap detaches its thread after invocation.
#[derive(Deserialize, Serialize)]
pub struct MonoPlan {
    pub assembly: String,
    /// Preferred scripting domain; use the root domain when it is absent.
    pub domain: String,
    pub namespace: String,
    pub class: String,
    pub method: String,
    /// Encoded by the platform for its managed entry to interpret.
    pub argument: String,
}

pub fn attach_directory() -> PathBuf {
    std::env::temp_dir().join("flint-bridge").join("attach")
}

/// The injector writes this file; the bootstrap reads and removes it once loaded.
pub fn plan_path(pid: u32) -> PathBuf {
    attach_directory().join(format!("{pid}.json"))
}

/// Startup failures are UTF-8 text. The injector clears an earlier report before
/// publishing a new plan; the bootstrap or its scheduled runtime work writes it.
pub fn error_path(pid: u32) -> PathBuf {
    attach_directory().join(format!("{pid}.error"))
}
