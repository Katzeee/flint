//! Windows loader entry point and loaded-module access.

use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::c_void;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

use anyhow::Result;
use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    FreeLibraryAndExitThread, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcess, GetCurrentProcessId};

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// Loader entry point. Do the minimum here — spawn a worker and return — so no
/// real work runs while the process holds the loader lock.
#[no_mangle]
pub extern "system" fn DllMain(
    module: HMODULE,
    reason: u32,
    _reserved: *mut core::ffi::c_void,
) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module, Ordering::Release);
        unsafe {
            let thread = CreateThread(null(), 0, Some(worker), null(), 0, null_mut());
            if !thread.is_null() {
                CloseHandle(thread);
            }
        }
    }
    1
}

/// Unloads the bootstrap when done: a library that stays loaded gets no new
/// `DLL_PROCESS_ATTACH` from a later injection, so a repeated attach would do nothing.
unsafe extern "system" fn worker(_parameter: *mut core::ffi::c_void) -> u32 {
    crate::run(GetCurrentProcessId());
    FreeLibraryAndExitThread(MODULE.load(Ordering::Acquire), 0)
}

/// Resolve an export by null-terminated name, returning its address.
pub(crate) unsafe fn export(module: HMODULE, name: &[u8]) -> Result<*const ()> {
    match GetProcAddress(module, name.as_ptr()) {
        Some(address) => Ok(address as *const ()),
        None => Err(anyhow::anyhow!(
            "host runtime is missing {}",
            String::from_utf8_lossy(&name[..name.len() - 1])
        )),
    }
}

/// Find a loaded module by an exported symbol, independently of its file name.
pub(crate) unsafe fn module_with_export(name: &CStr) -> Option<HMODULE> {
    let process = GetCurrentProcess();
    let mut needed = 0u32;
    if EnumProcessModules(process, null_mut(), 0, &mut needed) == 0 || needed == 0 {
        return None;
    }
    let count = needed as usize / size_of::<HMODULE>();
    let mut modules: Vec<HMODULE> = vec![null_mut(); count];
    if EnumProcessModules(process, modules.as_mut_ptr(), needed, &mut needed) == 0 {
        return None;
    }
    modules
        .into_iter()
        .find(|&module| !module.is_null() && GetProcAddress(module, name.as_ptr().cast()).is_some())
}

/// The base names of every module currently loaded in this process, for
/// diagnostics when the interpreter cannot be found.
pub(crate) unsafe fn list_modules() -> Vec<String> {
    let process = GetCurrentProcess();
    let mut needed = 0u32;
    if EnumProcessModules(process, null_mut(), 0, &mut needed) == 0 || needed == 0 {
        return vec![format!(
            "<enumeration failed: {}>",
            std::io::Error::last_os_error()
        )];
    }
    let count = needed as usize / size_of::<HMODULE>();
    let mut modules: Vec<HMODULE> = vec![null_mut(); count];
    if EnumProcessModules(process, modules.as_mut_ptr(), needed, &mut needed) == 0 {
        return vec![format!(
            "<enumeration failed: {}>",
            std::io::Error::last_os_error()
        )];
    }
    let mut names = vec![];
    for &module in &modules {
        let mut buffer = [0u16; 260];
        let length = GetModuleBaseNameW(process, module, buffer.as_mut_ptr(), buffer.len() as u32);
        if length != 0 {
            names.push(String::from_utf16_lossy(&buffer[..length as usize]));
        }
    }
    names
}

/// Find a loaded module by its file name.
pub(crate) unsafe fn find_module(name: &str) -> Option<HMODULE> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let module = GetModuleHandleW(wide.as_ptr());
    (!module.is_null()).then_some(module)
}
