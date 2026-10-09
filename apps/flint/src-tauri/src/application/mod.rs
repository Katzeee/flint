//! The product API shared by application frontends.
//!
//! Local host operations are associated functions that need neither a backend nor
//! its configuration. Backend queries use the control protocol and never start a
//! process implicitly. `start_backend`, `restart_backend`, and `attach` establish
//! a running backend when their operation requires one.

mod attach;
mod control;
mod hosts;
mod workflows;

use serde::Serialize;
use std::path::PathBuf;

pub use attach::AttachResult;
pub use control::{BackendStopped, Snapshot};
use flint_backend::config::Config;
pub use flint_contracts::protocol::{
    ExecuteRequest, ExecutionResult, ExecutionStatus, ExecutionView, Failure, FailureCode,
    GetExecutionResponse, GetWorkflowResponse, InstanceInfo, PingResponse, StartWorkflowRequest,
    StartWorkflowResponse, WorkflowSummary,
};
pub use flint_hosts::{ExportTarget, HostCandidate, HostKind};
pub use hosts::{ExportResult, HostInfo, Preview};

pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Serialize)]
pub struct ApplicationInfo {
    pub version: &'static str,
    pub control_endpoint: String,
    pub bridge_endpoint: String,
    pub state_dir: PathBuf,
    pub attach_supported: bool,
}

pub struct Application {
    config: Config,
}

impl Application {
    /// Loads the product runtime without creating files or starting a backend.
    pub fn load() -> Result<Self> {
        Ok(Self {
            config: Config::load()
                .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, error.as_ref()))?,
        })
    }

    pub fn info(&self) -> ApplicationInfo {
        let config = &self.config;
        ApplicationInfo {
            version: env!("CARGO_PKG_VERSION"),
            control_endpoint: format!("{}:{}", config.address, config.control_port),
            bridge_endpoint: format!("{}:{}", config.address, config.bridge_port),
            state_dir: config.state_dir.clone(),
            attach_supported: flint_hosts::attach_supported(),
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Failure::caused_by(FailureCode::InternalError, &error))?
}

#[cfg(test)]
mod tests;
