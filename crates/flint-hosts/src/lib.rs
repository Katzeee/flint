use serde::Serialize;
use std::ffi::OsString;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

mod attach;
mod window;
pub use attach::{attach, attach_error, AttachRequest, Runtime};
pub use window::{focus_application, host_info, HostInfo, WindowInfo, WindowPreview};

#[cfg(test)]
mod tests;

#[derive(Serialize)]
pub struct HostCandidate {
    pub pid: u32,
    pub host: &'static str,
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
        let Some(host) = host_kind(&name, process.cmd()) else {
            continue;
        };
        found.push(HostCandidate {
            pid: pid.as_u32(),
            host,
            executable: process
                .exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        });
    }
    found.sort_by_key(|p| p.pid);
    found
}

fn host_kind(name: &str, command: &[OsString]) -> Option<&'static str> {
    match name {
        "maya.exe" | "maya" => Some("maya"),
        "3dsmax.exe" => Some("max"),
        "blender.exe" | "blender" => Some("blender"),
        // Asset import workers run the editor executable but are not independent hosts.
        // Batch-mode editors remain discoverable even when they have no window.
        "unity.exe" if !is_unity_import_worker(command) => Some("unity"),
        _ => None,
    }
}

fn is_unity_import_worker(command: &[OsString]) -> bool {
    command
        .iter()
        .skip(1)
        .any(|arg| arg.eq_ignore_ascii_case("-assetImportWorker"))
        || command.windows(2).any(|pair| {
            pair[0].eq_ignore_ascii_case("-name")
                && pair[1]
                    .to_string_lossy()
                    .to_ascii_lowercase()
                    .starts_with("assetimportworker")
        })
}
