//! One list of host applications: local processes and registered instances, joined by instance ID.

use super::{Application, InstanceInfo, Result, blocking};
use flint_hosts::HostCandidate;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Ready,
    Connecting,
    NotConnected,
}

#[derive(Debug, Serialize, specta::Type)]
pub struct App {
    pub instance_id: String,
    /// A built-in host kind, or the type an independently loaded Bridge registered.
    pub host: String,
    /// The registered instance name; absent until a Bridge connects.
    pub name: Option<String>,
    /// Only meaningful on this machine when `local` is true.
    pub pid: u32,
    pub local: bool,
    pub connection: Connection,
    /// Present when discovery recognizes the process on this machine.
    pub executable: Option<String>,
    pub runtime_version: Option<String>,
    pub bridge_version: Option<String>,
}

/// Local processes whose instance ID cannot be derived have exited since discovery.
fn identify(candidates: Vec<HostCandidate>) -> Vec<(String, HostCandidate)> {
    candidates
        .into_iter()
        .filter_map(|candidate| {
            Some((
                flint_contracts::instance::id_for_process(candidate.pid).ok()?,
                candidate,
            ))
        })
        .collect()
}

fn is_local(instance: &InstanceInfo) -> bool {
    flint_contracts::instance::id_for_process(instance.pid).is_ok_and(|id| id == instance.instance_id)
}

pub(super) fn join(
    instances: Vec<InstanceInfo>,
    mut discovered: Vec<(String, HostCandidate)>,
    is_local: impl Fn(&InstanceInfo) -> bool,
) -> Vec<App> {
    let mut apps = Vec::with_capacity(instances.len() + discovered.len());
    for instance in instances {
        let candidate = discovered
            .iter()
            .position(|(id, _)| *id == instance.instance_id)
            .map(|index| discovered.swap_remove(index).1);
        apps.push(App {
            local: candidate.is_some() || is_local(&instance),
            connection: if instance.execution_ready {
                Connection::Ready
            } else {
                Connection::Connecting
            },
            executable: candidate.map(|candidate| candidate.executable),
            instance_id: instance.instance_id,
            host: instance.instance_type,
            name: Some(instance.instance_name),
            pid: instance.pid,
            runtime_version: Some(instance.runtime_version),
            bridge_version: Some(instance.bridge_version),
        });
    }
    apps.extend(discovered.into_iter().map(|(instance_id, candidate)| App {
        instance_id,
        host: candidate.host.to_string(),
        name: None,
        pid: candidate.pid,
        local: true,
        connection: Connection::NotConnected,
        executable: Some(candidate.executable),
        runtime_version: None,
        bridge_version: None,
    }));
    apps
}

/// Joins `instances` with the host processes discovered on this machine.
pub(super) async fn with_local(instances: Vec<InstanceInfo>, host: Option<String>) -> Result<Vec<App>> {
    blocking(move || {
        let candidates = flint_hosts::discover()
            .into_iter()
            .filter(|candidate| host.as_deref().is_none_or(|host| host == <&str>::from(candidate.host)))
            .collect();
        Ok(join(instances, identify(candidates), is_local))
    })
    .await
}

/// The application `instance` belongs to, as the list would show it.
pub(super) async fn of_instance(instance: InstanceInfo) -> Result<App> {
    blocking(move || {
        let candidate = flint_hosts::candidate(instance.pid).into_iter().collect();
        Ok(join(vec![instance], identify(candidate), is_local)
            .into_iter()
            .next()
            .expect("an instance yields its application"))
    })
    .await
}

impl Application {
    /// Requires a running backend; registered instances come first.
    pub async fn apps(&self, host: Option<String>) -> Result<Vec<App>> {
        with_local(self.instances(host.clone()).await?, host).await
    }
}
