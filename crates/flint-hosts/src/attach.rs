//! Attach: start a Bridge inside a running host by injecting the bootstrap.
//!
//! This is the injector that runs in flint's own process. It stages the host's
//! attach layout, has the host's platform turn its entry into a runtime plan, and
//! loads the bootstrap into the target, which executes that plan to start the Bridge.
//! Injection knows nothing about the backend; the caller confirms the Bridge
//! registered, or reads [`attach_error`] for why the injected side could not start it.

use crate::{
    HostKind,
    bridge::{Attach, bridge},
    layout::stage,
};
use anyhow::Result;
use flint_contracts::attach::{RuntimePlan, attach_directory, error_path, plan_path};
use strum::IntoEnumIterator;

pub struct AttachRequest {
    pub address: String,
    pub port: u16,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
#[error("attach is not implemented for host kind {0}")]
pub struct Unsupported(pub HostKind);

/// Only the Windows bootstrap can enter a host.
fn declaration(host: HostKind) -> Option<Attach> {
    if cfg!(windows) { bridge(host).attach } else { None }
}

pub fn attachment(host: HostKind) -> Result<Attach, Unsupported> {
    declaration(host).ok_or(Unsupported(host))
}

pub fn attach_supported() -> bool {
    HostKind::iter().any(|host| declaration(host).is_some())
}

/// Why the most recent attach to `pid` could not start or re-point its Bridge,
/// once the injected side has reported it. Absent while it is still working or
/// after it succeeded.
pub fn attach_error(pid: u32) -> Option<String> {
    std::fs::read_to_string(error_path(pid)).ok()
}

fn staging() -> std::path::PathBuf {
    attach_directory().join("staged")
}

/// Publish the host's startup instructions for the injected bootstrap.
fn write_plan(pid: u32, plan: &RuntimePlan) -> Result<()> {
    std::fs::create_dir_all(attach_directory())?;
    std::fs::write(plan_path(pid), serde_json::to_vec(plan)?)?;
    Ok(())
}

/// Inject the Bridge bootstrap into the host process `pid`.
///
/// Returns once the bootstrap library is loaded; the Bridge then connects
/// asynchronously and the caller confirms it through the backend.
impl Attach {
    pub fn inject(self, pid: u32, request: &AttachRequest) -> Result<()> {
        let root = stage(&staging(), &self.layout)?;
        let plan = self.entry.plan(&root, request, &error_path(pid));
        // A previous attempt's outcome must not be mistaken for this one's.
        match std::fs::remove_file(error_path(pid)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        write_plan(pid, &plan)?;
        match os::inject(pid) {
            Ok(()) => Ok(()),
            Err(error) => {
                // Leave no stale plan if the bootstrap never loaded to read it.
                let _ = std::fs::remove_file(plan_path(pid));
                Err(error)
            }
        }
    }
}

#[cfg(not(windows))]
mod os {
    pub fn inject(_pid: u32) -> anyhow::Result<()> {
        unreachable!("no host declares attach off Windows")
    }
}
#[cfg(windows)]
mod os {
    use super::*;
    use crate::layout::{BOOTSTRAP, Layout};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
    };
    use windows_sys::Win32::System::Threading::{
        CreateRemoteThread, GetExitCodeThread, IsWow64Process2, LPTHREAD_START_ROUTINE, OpenProcess,
        PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
        WaitForSingleObject,
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

    pub fn inject(pid: u32) -> Result<()> {
        let bootstrap = Layout::default().file("flint-bootstrap.dll", BOOTSTRAP);
        let bootstrap = stage(&staging(), &bootstrap)?.join("flint-bootstrap.dll");
        unsafe { inject_windows(pid, &bootstrap) }
    }

    unsafe fn inject_windows(pid: u32, bootstrap: &Path) -> Result<()> {
        let access = PROCESS_CREATE_THREAD
            | PROCESS_QUERY_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_WRITE
            | PROCESS_VM_READ;
        let process = OpenProcess(access, 0, pid);
        anyhow::ensure!(!process.is_null(), "cannot open process {pid} for attach");
        let process = Handle(process);

        // The bootstrap is built for flint's own 64-bit target; refuse a 32-bit
        // (WOW64) target rather than load a mismatched image. A process that
        // is not under WOW64 reports UNKNOWN (0).
        let mut process_machine = 0u16;
        let mut native_machine = 0u16;
        anyhow::ensure!(
            IsWow64Process2(process.0, &mut process_machine, &mut native_machine) != 0,
            "cannot determine the architecture of process {pid}"
        );
        anyhow::ensure!(
            process_machine == 0,
            "attach supports 64-bit hosts only; process {pid} is 32-bit"
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
        anyhow::ensure!(!remote.is_null(), "cannot allocate memory in process {pid}");
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
            "cannot write the bootstrap path into process {pid}"
        );

        // kernel32 loads at the same base in every process of a session, so the
        // local LoadLibraryW address is valid in the target.
        let kernel32 = GetModuleHandleW(wide("kernel32.dll".as_ref()).as_ptr());
        anyhow::ensure!(!kernel32.is_null(), "cannot locate kernel32");
        let load_library = GetProcAddress(kernel32, c"LoadLibraryW".as_ptr().cast());
        let start: LPTHREAD_START_ROUTINE = Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
        >(
            load_library.ok_or_else(|| anyhow::anyhow!("cannot locate LoadLibraryW"))?,
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
        anyhow::ensure!(!thread.is_null(), "cannot start the loader thread in process {pid}");
        let thread = Handle(thread);

        // 30s covers a busy host; the loader itself is quick once scheduled.
        anyhow::ensure!(
            WaitForSingleObject(thread.0, 30_000) == WAIT_OBJECT_0,
            "the loader thread in process {pid} did not finish"
        );
        let mut exit = 0u32;
        anyhow::ensure!(
            GetExitCodeThread(thread.0, &mut exit) != 0,
            "cannot read the loader result from process {pid}"
        );
        // LoadLibraryW returns the module handle, truncated to 32 bits here; zero
        // means the bootstrap failed to load.
        anyhow::ensure!(exit != 0, "the host rejected the Bridge bootstrap");
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
