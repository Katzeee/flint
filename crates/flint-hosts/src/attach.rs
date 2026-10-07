//! Attach: start a Bridge inside a running host by injecting the bootstrap.
//!
//! This is the injector that runs in flint's own process. It loads the small
//! bootstrap library into the target and leaves a config file the bootstrap
//! reads to start the Bridge. It performs pure operating-system injection and
//! knows nothing about the backend; the caller confirms the Bridge registered,
//! or reads [`attach_error`] for why the injected side could not start it.

use std::path::PathBuf;

use anyhow::Result;
use flint_contracts::host::HostKind;
use serde::Serialize;

/// The host runtime the bootstrap drives once injected.
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    Cpython,
    /// A managed runtime (Mono today, CoreCLR later), resolved in-process.
    Dotnet,
}

/// Everything the injector needs to place a Bridge into one host process.
pub struct AttachRequest {
    pub host: HostKind,
    pub runtime: Runtime,
    /// The bootstrap library flint injects into the host.
    pub bootstrap: PathBuf,
    /// The Bridge package the host runtime loads: the Python ZIP or a managed assembly.
    pub payload: PathBuf,
    /// The native core the managed runtime loads itself; unused for CPython.
    pub core: Option<PathBuf>,
    pub address: String,
    pub port: u16,
    pub name: String,
}

/// The config the injector writes for the injected bootstrap to read once.
#[derive(Serialize)]
struct AttachConfig<'a> {
    runtime: Runtime,
    host: HostKind,
    address: &'a str,
    port: u16,
    name: &'a str,
    payload: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    core_path: Option<String>,
}

fn attach_directory() -> PathBuf {
    std::env::temp_dir().join("flint-bridge").join("attach")
}

fn config_path(pid: u32) -> PathBuf {
    attach_directory().join(format!("{pid}.json"))
}

fn error_path(pid: u32) -> PathBuf {
    attach_directory().join(format!("{pid}.error"))
}

/// Why the most recent attach to `pid` could not start or re-point its Bridge,
/// once the injected side has reported it. Absent while it is still working or
/// after it succeeded.
pub fn attach_error(pid: u32) -> Option<String> {
    std::fs::read_to_string(error_path(pid)).ok()
}

/// Write the per-process config the bootstrap reads after it is injected.
fn write_config(pid: u32, request: &AttachRequest) -> Result<()> {
    let directory = attach_directory();
    std::fs::create_dir_all(&directory)?;
    let payload = request
        .payload
        .canonicalize()
        .unwrap_or_else(|_| request.payload.clone());
    let core_path = request.core.as_ref().map(|core| {
        core.canonicalize()
            .unwrap_or_else(|_| core.clone())
            .display()
            .to_string()
    });
    let config = AttachConfig {
        runtime: request.runtime,
        host: request.host,
        address: &request.address,
        port: request.port,
        name: &request.name,
        payload: payload.display().to_string(),
        core_path,
    };
    std::fs::write(config_path(pid), serde_json::to_vec(&config)?)?;
    Ok(())
}

/// Inject the Bridge bootstrap into the host process `pid`.
///
/// Returns once the bootstrap library is loaded; the Bridge then connects
/// asynchronously and the caller confirms it through the backend.
pub fn attach(pid: u32, request: &AttachRequest) -> Result<()> {
    anyhow::ensure!(
        request.bootstrap.is_file(),
        "Bridge bootstrap is missing: {}",
        request.bootstrap.display()
    );
    // A previous attempt's outcome must not be mistaken for this one's.
    match std::fs::remove_file(error_path(pid)) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    write_config(pid, request)?;
    match platform::inject(pid, &request.bootstrap) {
        Ok(()) => Ok(()),
        Err(error) => {
            // Leave no stale config if the bootstrap never loaded to read it.
            let _ = std::fs::remove_file(config_path(pid));
            Err(error)
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use super::*;

    pub fn inject(_pid: u32, _bootstrap: &std::path::Path) -> Result<()> {
        anyhow::bail!("Attach is only supported on Windows")
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, IsWow64Process2, OpenProcess, WaitForSingleObject,
        LPTHREAD_START_ROUTINE, PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION,
        PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    };

    /// Owns a handle and closes it on drop so early returns cannot leak it.
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.0) };
        }
    }

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn inject(pid: u32, bootstrap: &Path) -> Result<()> {
        unsafe { inject_windows(pid, bootstrap) }
    }

    unsafe fn inject_windows(pid: u32, bootstrap: &Path) -> Result<()> {
        let access = PROCESS_CREATE_THREAD
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_WRITE
            | PROCESS_VM_READ;
        let process = OpenProcess(access, 0, pid);
        anyhow::ensure!(!process.is_null(), "Cannot open process {pid} for attach");
        let process = Handle(process);

        // The bootstrap is x64; refuse a 32-bit (WOW64) target rather than
        // load a mismatched image. A native x64 process reports UNKNOWN (0).
        let mut process_machine = 0u16;
        let mut native_machine = 0u16;
        anyhow::ensure!(
            IsWow64Process2(process.0, &mut process_machine, &mut native_machine) != 0,
            "Cannot determine the architecture of process {pid}"
        );
        anyhow::ensure!(
            process_machine == 0,
            "Attach supports 64-bit hosts only; process {pid} is 32-bit"
        );

        let path = wide(bootstrap.as_os_str());
        let bytes = std::mem::size_of_val(path.as_slice());
        let remote = VirtualAllocEx(
            process.0,
            std::ptr::null(),
            bytes,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        anyhow::ensure!(!remote.is_null(), "Cannot allocate memory in process {pid}");
        let remote = RemoteMemory {
            process: process.0,
            address: remote,
        };
        anyhow::ensure!(
            WriteProcessMemory(
                process.0,
                remote.address,
                path.as_ptr().cast(),
                bytes,
                std::ptr::null_mut(),
            ) != 0,
            "Cannot write the bootstrap path into process {pid}"
        );

        // kernel32 loads at the same base in every process of a session, so the
        // local LoadLibraryW address is valid in the target.
        let kernel32 = GetModuleHandleW(wide("kernel32.dll".as_ref()).as_ptr());
        anyhow::ensure!(!kernel32.is_null(), "Cannot locate kernel32");
        let load_library = GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast());
        let start: LPTHREAD_START_ROUTINE = Some(std::mem::transmute::<
            _,
            unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
        >(
            load_library.ok_or_else(|| anyhow::anyhow!("Cannot locate LoadLibraryW"))?,
        ));

        let thread = CreateRemoteThread(
            process.0,
            std::ptr::null(),
            0,
            start,
            remote.address,
            0,
            std::ptr::null_mut(),
        );
        anyhow::ensure!(
            !thread.is_null(),
            "Cannot start the loader thread in process {pid}"
        );
        let thread = Handle(thread);

        // 30s covers a busy host; the loader itself is quick once scheduled.
        anyhow::ensure!(
            WaitForSingleObject(thread.0, 30_000) == WAIT_OBJECT_0,
            "The loader thread in process {pid} did not finish"
        );
        let mut exit = 0u32;
        anyhow::ensure!(
            GetExitCodeThread(thread.0, &mut exit) != 0,
            "Cannot read the loader result from process {pid}"
        );
        // LoadLibraryW returns the module handle, truncated to 32 bits here; zero
        // means the bootstrap failed to load.
        anyhow::ensure!(exit != 0, "The host rejected the Bridge bootstrap");
        Ok(())
    }

    /// Frees the remote allocation on drop.
    struct RemoteMemory {
        process: HANDLE,
        address: *mut core::ffi::c_void,
    }
    impl Drop for RemoteMemory {
        fn drop(&mut self) {
            unsafe { VirtualFreeEx(self.process, self.address, 0, MEM_RELEASE) };
        }
    }
}
