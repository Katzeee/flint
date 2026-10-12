//! An instance is one host process lifetime, independent of Bridge objects,
//! connection settings, backend restarts, and managed domains.
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

/// Return a local process's instance ID, stable for that process's lifetime.
pub fn id_for_process(pid: u32) -> Result<String> {
    let machine = machine_uid::get().map_err(|error| anyhow::anyhow!("cannot read machine identity: {error}"))?;
    anyhow::ensure!(!machine.trim().is_empty(), "machine identity is empty");
    let start_marker = process_start_marker(pid).context("cannot read process start marker")?;

    let mut hash = Sha256::new();
    hash.update(b"flint-instance-v1\0");
    hash.update((machine.len() as u64).to_be_bytes());
    hash.update(machine.as_bytes());
    hash.update(pid.to_be_bytes());
    hash.update(start_marker);
    Ok(URL_SAFE_NO_PAD.encode(&hash.finalize()[..12]))
}

// Opaque bytes distinguishing process starts for the same PID, not a portable timestamp.
#[cfg(windows)]
fn process_start_marker(pid: u32) -> Result<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    };
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let mut created: FILETIME = unsafe { std::mem::zeroed() };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    let success = unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) };
    let error = std::io::Error::last_os_error();
    unsafe {
        windows_sys::Win32::Foundation::CloseHandle(process);
    }
    if success == 0 {
        return Err(error.into());
    }
    Ok(
        ((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
            .to_be_bytes()
            .to_vec(),
    )
}

#[cfg(target_os = "linux")]
fn process_start_marker(pid: u32) -> Result<Vec<u8>> {
    // /proc stat's comm field can contain spaces and parentheses. Field 22 is
    // starttime in clock ticks since boot; include the boot ID across reboots.
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let (_, fields) = stat.rsplit_once(')').context("invalid process stat")?;
    let ticks: u64 = fields
        .split_whitespace()
        .nth(19)
        .context("missing process starttime")?
        .parse()?;
    let mut value = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .as_bytes()
        .to_vec();
    value.extend_from_slice(&ticks.to_be_bytes());
    Ok(value)
}

#[cfg(target_os = "macos")]
fn process_start_marker(pid: u32) -> Result<Vec<u8>> {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info) as i32;
    let read = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    anyhow::ensure!(
        read == size,
        "cannot read process information: {}",
        std::io::Error::last_os_error()
    );
    let mut value = info.pbi_start_tvsec.to_be_bytes().to_vec();
    value.extend_from_slice(&info.pbi_start_tvusec.to_be_bytes());
    Ok(value)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn process_start_marker(_pid: u32) -> Result<Vec<u8>> {
    anyhow::bail!("process identity is unsupported on this platform")
}
