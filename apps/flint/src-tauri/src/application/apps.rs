//! Host applications on the backend's machine.
use super::{Application, Result};
use flint_contracts::protocol::{envelope::Payload, *};
use flint_hosts::HostKind;

impl Application {
    pub async fn apps(&self, host: Option<String>) -> Result<Vec<App>> {
        self.query(
            Payload::ListAppsRequest(ListAppsRequest { host }),
            |response| match response {
                Payload::ListAppsResponse(response) => Some(response.apps),
                _ => None,
            },
        )
        .await
    }

    pub async fn app_info(&self, pid: u32, preview: bool) -> Result<GetAppInfoResponse> {
        self.query(
            Payload::GetAppInfoRequest(GetAppInfoRequest { pid, preview }),
            |response| match response {
                Payload::GetAppInfoResponse(response) => Some(response),
                _ => None,
            },
        )
        .await
    }

    pub async fn focus_app(&self, pid: u32) -> Result<()> {
        if let Some(backend) = self.backend_status().await? {
            flint_hosts::allow_focus(backend.pid);
        }
        self.query(
            Payload::FocusAppRequest(FocusAppRequest { pid }),
            |response| match response {
                Payload::FocusAppResponse(_) => Some(()),
                _ => None,
            },
        )
        .await
    }

    pub async fn attach(&self, pid: u32, host_kind: Option<HostKind>, name: Option<String>) -> Result<App> {
        self.query(
            Payload::AttachRequest(AttachRequest {
                pid,
                host_kind: host_kind.map(|host| host.to_string()),
                name,
            }),
            |response| match response {
                Payload::AttachResponse(response) => response.app,
                _ => None,
            },
        )
        .await
    }
}
