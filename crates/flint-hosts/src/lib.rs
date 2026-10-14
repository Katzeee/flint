mod host;
pub use host::HostKind;
use serde::Serialize;
use strum::IntoEnumIterator;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

mod attach;
mod bridge;
mod layout;
mod platform;
mod window;
pub use attach::{Unsupported, attach_error, attach_supported, attachment};
pub use bridge::Attach;
pub use bridge::{ExportTarget, export};
pub use platform::Platform;
pub use window::{HostInfo, WindowInfo, WindowPreview, focus_application, host_info};

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("process {0} is not a recognized host")]
    NotAHost(u32),
    #[error(transparent)]
    Platform(#[from] anyhow::Error),
}

#[derive(Serialize)]
#[cfg_attr(feature = "typescript", derive(specta::Type))]
pub struct HostCandidate {
    pub pid: u32,
    pub host: HostKind,
    pub executable: String,
}

/// Process discovery does not imply that a bridge is connected or injectable.
pub fn discover() -> Vec<HostCandidate> {
    collect(ProcessesToUpdate::All)
}

pub fn candidate(pid: u32) -> Option<HostCandidate> {
    collect(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]))
        .into_iter()
        .find(|candidate| candidate.pid == pid)
}

fn collect(processes: ProcessesToUpdate<'_>) -> Vec<HostCandidate> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        processes,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::OnlyIfNotSet)
            .with_cmd(UpdateKind::OnlyIfNotSet),
    );
    let mut found = vec![];
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        let Some(host) = HostKind::iter().find(|host| host.matches_process(&name, process.cmd())) else {
            continue;
        };
        found.push(HostCandidate {
            pid: pid.as_u32(),
            host,
            executable: process.exe().map(|p| p.display().to_string()).unwrap_or_default(),
        });
    }
    found.sort_by_key(|p| p.pid);
    found
}
