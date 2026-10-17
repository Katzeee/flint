//! The Bridge token Bridges present, and Bridge packages written to the caller's file system.
use super::{Application, Failure, FailureCode, Result};
use flint_contracts::protocol::{envelope::Payload, *};
use flint_hosts::ExportTarget;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub path: PathBuf,
    pub version: &'static str,
}

impl Application {
    pub async fn bridge_token(&self) -> Result<String> {
        self.query(
            Payload::GetBridgeTokenRequest(GetBridgeTokenRequest {}),
            |response| match response {
                Payload::GetBridgeTokenResponse(response) => Some(response.token),
                _ => None,
            },
        )
        .await
    }

    pub fn export_targets() -> Vec<ExportTarget> {
        ExportTarget::available().collect()
    }

    pub async fn export_bridge(target: ExportTarget, output: Option<PathBuf>) -> Result<ExportResult> {
        let path = tokio::task::spawn_blocking(move || flint_hosts::export(target, output.as_deref()))
            .await
            .map_err(|error| Failure::caused_by(FailureCode::InternalError, &error))?
            .map_err(|error| Failure::caused_by(FailureCode::CommandFailed, error.as_ref()))?;
        Ok(ExportResult {
            path,
            version: env!("CARGO_PKG_VERSION"),
        })
    }
}
