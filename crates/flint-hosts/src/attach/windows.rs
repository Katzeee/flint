//! Windows bootstrap loading and remote invocation.
use crate::layout::stage;
use crate::layout::{BOOTSTRAP, Layout};
use anyhow::Result;
use flint_contracts::{attach::BootstrapRequest, config::StateDir};
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows_sys::Win32::Foundation::{CloseHandle, FreeLibrary, HANDLE, HMODULE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows_sys::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows_sys::Win32::System::ProcessStatus::{EnumProcessModulesEx, GetModuleFileNameExW, LIST_MODULES_ALL};
use windows_sys::Win32::System::Threading::{
    CreateRemoteThread, GetExitCodeThread, IsWow64Process2, LPTHREAD_START_ROUTINE, OpenProcess, PROCESS_CREATE_THREAD,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE, WaitForSingleObject,
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

pub(super) fn inject(pid: u32, state: &StateDir, request: &BootstrapRequest) -> Result<()> {
    let bootstrap = Layout::default().file("flint-bootstrap.dll", BOOTSTRAP);
    let bootstrap = stage(&state.attach_staging(), &bootstrap)?.join("flint-bootstrap.dll");
    unsafe { inject_windows(pid, &bootstrap, request) }
}

unsafe fn inject_windows(pid: u32, bootstrap: &Path, request: &BootstrapRequest) -> Result<()> {
    let access =
        PROCESS_CREATE_THREAD | PROCESS_QUERY_INFORMATION | PROCESS_VM_OPERATION | PROCESS_VM_WRITE | PROCESS_VM_READ;
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
    // DllMain only records its module handle; loading locally resolves the
    // exported entry offset without executing an attach plan.
    let local = LoadLibraryW(path.as_ptr());
    anyhow::ensure!(
        !local.is_null(),
        "cannot load bootstrap exports: {}",
        std::io::Error::last_os_error()
    );
    let entry = GetProcAddress(local, c"flint_bootstrap_start".as_ptr().cast());
    let offset = entry.map(|entry| entry as usize - local as usize);
    FreeLibrary(local);
    let offset = offset.ok_or_else(|| anyhow::anyhow!("bootstrap has no start entry"))?;

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

    let path_bytes = std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), std::mem::size_of_val(path.as_slice()));
    remote_call(process.0, start, path_bytes)?;
    // A remote thread exit code is only 32 bits; obtain the full module base
    // from the target's module list instead of treating that code as HMODULE.
    let module = remote_module(process.0, bootstrap)?;
    let start: LPTHREAD_START_ROUTINE = Some(std::mem::transmute::<
        usize,
        unsafe extern "system" fn(*mut core::ffi::c_void) -> u32,
    >(module as usize + offset));
    let mut payload = serde_json::to_vec(request)?;
    payload.push(0);
    let exit = remote_call(process.0, start, &payload)?;
    if exit != 0 {
        let report = std::fs::read_to_string(&request.error_path).unwrap_or_else(|_| {
            format!(
                "bootstrap failed with status {exit}; error report: {}",
                request.error_path.display()
            )
        });
        anyhow::bail!("{report}");
    }
    Ok(())
}

unsafe fn remote_module(process: HANDLE, path: &Path) -> Result<HMODULE> {
    let mut modules = vec![std::ptr::null_mut(); 256];
    loop {
        let capacity = std::mem::size_of_val(modules.as_slice()) as u32;
        let mut needed = 0;
        anyhow::ensure!(
            EnumProcessModulesEx(process, modules.as_mut_ptr(), capacity, &mut needed, LIST_MODULES_ALL) != 0,
            "cannot enumerate host modules: {}",
            std::io::Error::last_os_error()
        );
        if needed > capacity {
            modules.resize(needed as usize / std::mem::size_of::<HMODULE>(), std::ptr::null_mut());
            continue;
        }
        modules.truncate(needed as usize / std::mem::size_of::<HMODULE>());
        break;
    }
    let expected = std::fs::canonicalize(path)?;
    let mut name = vec![0u16; 32768];
    for module in modules {
        let length = GetModuleFileNameExW(process, module, name.as_mut_ptr(), name.len() as u32);
        if length != 0 {
            let actual = std::path::PathBuf::from(String::from_utf16_lossy(&name[..length as usize]));
            if std::fs::canonicalize(actual).is_ok_and(|actual| actual == expected) {
                return Ok(module);
            }
        }
    }
    anyhow::bail!("host did not load bootstrap {}", path.display())
}

unsafe fn remote_call(process: HANDLE, start: LPTHREAD_START_ROUTINE, payload: &[u8]) -> Result<u32> {
    let address = VirtualAllocEx(
        process,
        std::ptr::null(),
        payload.len(),
        MEM_COMMIT | MEM_RESERVE,
        PAGE_READWRITE,
    );
    anyhow::ensure!(
        !address.is_null(),
        "cannot allocate bootstrap arguments: {}",
        std::io::Error::last_os_error()
    );
    let remote = RemoteMemory { process, address };
    anyhow::ensure!(
        WriteProcessMemory(
            process,
            address,
            payload.as_ptr().cast(),
            payload.len(),
            std::ptr::null_mut()
        ) != 0,
        "cannot write bootstrap arguments: {}",
        std::io::Error::last_os_error()
    );
    let thread = CreateRemoteThread(process, std::ptr::null(), 0, start, address, 0, std::ptr::null_mut());
    anyhow::ensure!(
        !thread.is_null(),
        "cannot start bootstrap thread: {}",
        std::io::Error::last_os_error()
    );
    let thread = Handle(thread);
    if WaitForSingleObject(thread.0, 30_000) != WAIT_OBJECT_0 {
        // The host may still read these bytes. They live until process exit
        // if completion cannot be confirmed; freeing them would be unsafe.
        std::mem::forget(remote);
        anyhow::bail!("bootstrap thread did not finish");
    }
    let mut exit = 0;
    anyhow::ensure!(
        GetExitCodeThread(thread.0, &mut exit) != 0,
        "cannot read bootstrap thread result"
    );
    Ok(exit)
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
