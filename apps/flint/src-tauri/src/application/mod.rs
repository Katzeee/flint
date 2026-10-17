//! The product API shared by application frontends.
//!
//! Operations on a machine's host applications and workflows are backend requests
//! over the control protocol and never start a process implicitly; `start_backend`
//! establishes one. Bridge exports write to the caller's file system.

mod apps;
mod bridge;
mod control;
mod workflows;

use serde::Serialize;
use std::path::PathBuf;

pub use bridge::ExportResult;
pub use control::{BackendStopped, Snapshot};
use flint_contracts::config::Config;
pub use flint_contracts::protocol::{
    App, ExecuteRequest, ExecuteResponse, Execution, ExecutionStatus, ExecutionView, Failure, FailureCode,
    GetAppInfoResponse, PingResponse, StartWorkflowRequest, StartWorkflowResponse, Workflow, WorkflowSummary,
};
pub use flint_hosts::{ExportTarget, HostKind};
pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Serialize, specta::Type)]
pub struct ApplicationInfo {
    pub version: &'static str,
    pub control_endpoint: String,
    pub state_dir: PathBuf,
}

pub struct Application {
    config: Config,
}

impl Application {
    /// Loads the product runtime without creating files or starting a backend.
    pub fn load() -> Result<Self> {
        Ok(Self {
            config: Config::load().map_err(|error| Failure::caused_by(FailureCode::CommandFailed, error.as_ref()))?,
        })
    }

    pub fn info(&self) -> ApplicationInfo {
        let config = &self.config;
        ApplicationInfo {
            version: env!("CARGO_PKG_VERSION"),
            control_endpoint: config.endpoints.control.to_string(),
            state_dir: config.state.root().to_path_buf(),
        }
    }
}

#[cfg(test)]
mod tests;
