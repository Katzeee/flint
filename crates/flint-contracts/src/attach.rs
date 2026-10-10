//! Startup instructions shared by the injector
//! and the injected bootstrap. Plans name runtime entries, never hosts.

use serde::{Deserialize, Serialize};

/// Prepared by a platform from its host's entry and consumed once by the injected bootstrap.
#[derive(Deserialize, Serialize)]
#[serde(tag = "runtime", rename_all = "snake_case")]
pub enum RuntimePlan {
    Cpython(CpythonPlan),
    Mono(MonoPlan),
}

/// Python source the bootstrap runs once under the GIL on its injected thread.
/// The source returns promptly and reports later failures through
/// the error path supplied by the injector itself.
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

/// NUL-terminated JSON passed to the bootstrap's remote entry point.
/// The injector resolves both paths; the host never reconstructs them.
#[derive(Deserialize, Serialize)]
pub struct BootstrapRequest {
    pub plan_path: std::path::PathBuf,
    pub error_path: std::path::PathBuf,
}
