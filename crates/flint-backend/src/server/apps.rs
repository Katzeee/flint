//! Host applications: registrations joined with the host candidates discovered on this machine by instance ID.
use super::*;
use flint_contracts::{host_settings::HostSettings, instance::id_for_process};
use flint_hosts::{HostCandidate, HostError, HostKind, WindowPreview};

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Result<T, Failure> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| Failure::caused_by(FailureCode::InternalError, &error))
}

fn host_failure(error: HostError) -> Failure {
    let code = match error {
        HostError::NotAHost(_) => FailureCode::InvalidArguments,
        HostError::Platform(_) => FailureCode::CommandFailed,
    };
    Failure::caused_by(code, &error)
}

impl BackendHandle {
    pub(crate) fn registered(&self) -> Vec<App> {
        let state = self.0.state.lock().unwrap();
        let mut apps: Vec<_> = state
            .sessions
            .values()
            .map(|session| {
                let registration = &session.registration;
                App {
                    instance_id: registration.instance_id.clone(),
                    host: registration.host.clone(),
                    pid: registration.pid,
                    registration: Some(Registration {
                        name: registration.name.clone(),
                        runtime_version: registration.runtime_version.clone(),
                        bridge_version: registration.bridge_version.clone(),
                        execution_ready: session.sender.is_some(),
                    }),
                    ..Default::default()
                }
            })
            .collect();
        apps.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        apps
    }
}

/// Processes whose instance ID cannot be derived have exited since discovery.
fn identify(candidates: Vec<HostCandidate>) -> Vec<(String, HostCandidate)> {
    candidates
        .into_iter()
        .filter_map(|candidate| Some((id_for_process(candidate.pid).ok()?, candidate)))
        .collect()
}

fn is_local(app: &App) -> bool {
    id_for_process(app.pid).is_ok_and(|id| id == app.instance_id)
}

fn discovered_as(candidate: HostCandidate) -> Candidate {
    Candidate {
        executable: candidate.executable,
        attachable: flint_hosts::attachment(candidate.host).is_ok(),
    }
}

fn join(
    registered: Vec<App>,
    mut discovered: Vec<(String, HostCandidate)>,
    is_local: impl Fn(&App) -> bool,
) -> Vec<App> {
    let mut apps = Vec::with_capacity(registered.len() + discovered.len());
    for mut app in registered {
        let candidate = discovered
            .iter()
            .position(|(id, _)| *id == app.instance_id)
            .map(|index| discovered.swap_remove(index).1);
        app.local = candidate.is_some() || is_local(&app);
        app.candidate = candidate.map(discovered_as);
        apps.push(app);
    }
    apps.extend(discovered.into_iter().map(|(instance_id, candidate)| App {
        instance_id,
        host: candidate.host.to_string(),
        pid: candidate.pid,
        local: true,
        registration: None,
        candidate: Some(discovered_as(candidate)),
    }));
    apps
}

pub(super) async fn list(backend: &BackendHandle, host: Option<String>) -> Result<Vec<App>, Failure> {
    let registered = backend.registered();
    blocking(move || {
        let mut apps = join(registered, identify(flint_hosts::discover()), is_local);
        // A registration and its process can name different hosts; the joined app names one.
        apps.retain(|app| host.as_ref().is_none_or(|host| *host == app.host));
        apps
    })
    .await
}

pub(super) async fn info(backend: &BackendHandle, request: GetAppInfoRequest) -> Result<GetAppInfoResponse, Failure> {
    let pid = request.pid;
    let info = flint_hosts::candidate_info(pid, request.preview)
        .await
        .map_err(host_failure)?;
    let registered = backend.registered();
    let app = blocking(move || {
        let instance_id = id_for_process(info.candidate.pid).ok()?;
        let registered = registered
            .into_iter()
            .filter(|app| app.instance_id == instance_id)
            .collect();
        join(registered, vec![(instance_id, info.candidate)], is_local).pop()
    })
    .await?
    .ok_or_else(|| Failure::with_message(FailureCode::InvalidArguments, format!("process {pid} has exited")))?;
    Ok(GetAppInfoResponse {
        app: Some(app),
        window: info.window.map(|window| WindowInfo {
            title: window.title,
            minimized: window.minimized,
        }),
        preview: info.preview.map(|preview| match preview {
            WindowPreview::Png(png) => get_app_info_response::Preview::Png(png),
            WindowPreview::Unavailable(reason) => get_app_info_response::Preview::UnavailableReason(reason),
        }),
    })
}

pub(super) async fn focus(pid: u32) -> Result<(), Failure> {
    blocking(move || flint_hosts::focus_application(pid))
        .await?
        .map_err(host_failure)
}

/// Injects the Bridge and answers once it has connected with the requested settings.
pub(super) async fn attach(backend: &BackendHandle, request: AttachRequest) -> Result<App, Failure> {
    let pid = request.pid;
    let candidate = blocking(move || flint_hosts::candidate(pid)).await?;
    let host: HostKind = match (request.host_kind, &candidate) {
        (Some(kind), _) => kind
            .parse()
            .map_err(|_| Failure::with_message(FailureCode::InvalidArguments, format!("unknown host kind {kind}")))?,
        (None, Some(candidate)) => candidate.host,
        (None, None) => {
            return Err(Failure::with_message(
                FailureCode::InvalidArguments,
                format!("process {pid} is not a recognized host; specify its host kind"),
            ));
        }
    };
    let attach =
        flint_hosts::attachment(host).map_err(|error| Failure::caused_by(FailureCode::InvalidArguments, &error))?;
    let expected_id = blocking(move || id_for_process(pid))
        .await?
        .map_err(|error| Failure::caused_by(FailureCode::AttachFailed, error.as_ref()))?;
    let name = request.name.unwrap_or_else(|| host.to_string());
    let config = &backend.0.config;
    let target = config.endpoints.bridge_target();
    let settings = HostSettings {
        address: target.ip().to_string(),
        port: target.port(),
        name,
        token: String::new(),
    };
    let state = config.state.clone();
    blocking(move || attach.inject(pid, &state, settings))
        .await?
        .map_err(|error| {
            Failure::caused_by(
                FailureCode::AttachFailed,
                error.context("cannot inject the Bridge").as_ref(),
            )
        })?;
    let deadline = tokio::time::Instant::now() + config.timeout;
    // A Bridge already in the process keeps its registration until the new settings take
    // effect, so only the injected side's report says whether this attach succeeded.
    loop {
        match flint_hosts::attach_outcome(&config.state, pid) {
            Some(Ok(())) => break,
            Some(Err(reason)) => return Err(Failure::with_message(FailureCode::AttachFailed, reason)),
            None if tokio::time::Instant::now() >= deadline => return Err(Failure::new(FailureCode::AttachTimeout)),
            None => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
    let app = backend
        .registered()
        .into_iter()
        .find(|app| app.instance_id == expected_id)
        .ok_or_else(|| Failure::with_message(FailureCode::AttachFailed, "the Bridge disconnected after attaching"))?;
    let discovered = candidate
        .map(|candidate| (expected_id, candidate))
        .into_iter()
        .collect();
    blocking(move || join(vec![app], discovered, is_local).remove(0)).await
}

#[cfg(test)]
mod tests;
