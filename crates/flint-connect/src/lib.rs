use serde::Serialize;
use sysinfo::System;

#[derive(Serialize)]
pub struct HostCandidate {
    pub pid: u32,
    pub host: &'static str,
    pub executable: String,
    pub attach_supported: bool,
}

/// Process discovery does not imply that a bridge is connected or injectable.
pub fn discover() -> Vec<HostCandidate> {
    let system = System::new_all();
    let mut found = vec![];
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        let host = match name.as_str() {
            "maya.exe" | "maya" => "maya",
            "3dsmax.exe" => "max",
            "blender.exe" | "blender" => "blender",
            "unity.exe" => "unity",
            _ => continue,
        };
        found.push(HostCandidate {
            pid: pid.as_u32(),
            host,
            executable: process
                .exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            attach_supported: false,
        });
    }
    found.sort_by_key(|p| p.pid);
    found
}
