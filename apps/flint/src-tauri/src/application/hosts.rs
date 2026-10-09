//! Local host inspection, focus, previews, and Bridge exports.
use super::{blocking, Application, Failure, FailureCode, Result};
use base64::Engine;
use flint_hosts::{ExportTarget, HostCandidate, HostError, WindowInfo, WindowPreview};
use serde::Serialize;
use std::path::PathBuf;

fn host_failure(error: HostError) -> Failure {
    let code = match error {
        HostError::NotAHost(_) => FailureCode::InvalidArguments,
        HostError::Platform(_) => FailureCode::CommandFailed,
    };
    Failure::caused_by(code, &error)
}

#[derive(Serialize, specta::Type)]
pub struct HostInfo {
    #[serde(flatten)]
    pub candidate: HostCandidate,
    pub window: Option<WindowInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Preview>,
}

#[derive(Serialize, specta::Type)]
#[serde(untagged)]
pub enum Preview {
    Image { image: String },
    Unavailable { unavailable_reason: String },
}

impl From<flint_hosts::HostInfo> for HostInfo {
    fn from(info: flint_hosts::HostInfo) -> Self {
        Self {
            candidate: info.candidate,
            window: info.window,
            preview: info.preview.map(|preview| match preview {
                WindowPreview::Png(png) => Preview::Image {
                    image: format!(
                        "data:image/png;base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(png)
                    ),
                },
                WindowPreview::Unavailable(reason) => Preview::Unavailable {
                    unavailable_reason: reason,
                },
            }),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub path: PathBuf,
    pub version: &'static str,
}

impl Application {
    pub async fn hosts() -> Result<Vec<HostCandidate>> {
        blocking(|| Ok(flint_hosts::discover())).await
    }

    pub async fn host_info(pid: u32, preview: bool) -> Result<HostInfo> {
        flint_hosts::host_info(pid, preview)
            .await
            .map(Into::into)
            .map_err(host_failure)
    }

    pub async fn focus_application(pid: u32) -> Result<()> {
        blocking(move || flint_hosts::focus_application(pid).map_err(host_failure)).await
    }

    pub fn export_targets() -> Vec<ExportTarget> {
        ExportTarget::available().collect()
    }

    pub async fn export_bridge(
        target: ExportTarget,
        output: Option<PathBuf>,
    ) -> Result<ExportResult> {
        blocking(move || {
            Ok(ExportResult {
                path: flint_hosts::export(target, output.as_deref()).map_err(|error| {
                    Failure::caused_by(FailureCode::CommandFailed, error.as_ref())
                })?,
                version: env!("CARGO_PKG_VERSION"),
            })
        })
        .await
    }
}
