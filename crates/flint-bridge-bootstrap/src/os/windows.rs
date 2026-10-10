//! Windows loader entry point and loaded-module access.

use std::ffi::CStr;
use std::mem::size_of;
use std::os::raw::c_void;
use std::ptr::null_mut;
use std::sync::atomic::{AtomicPtr, Ordering};

use anyhow::Result;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{FreeLibraryAndExitThread, GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

static MODULE: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

/// Loader entry point; attach runs only through the explicit start export.
#[no_mangle]
pub extern "system" fn DllMain(module: HMODULE, reason: u32, _reserved: *mut c_void) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        MODULE.store(module, Ordering::Release);
    }
    1
}

/// The injector owns the NUL-terminated request until this thread finishes.
#[no_mangle]
pub unsafe extern "system" fn flint_bootstrap_start(parameter: *mut c_void) -> u32 {
    let exit = match serde_json::from_slice(CStr::from_ptr(parameter.cast()).to_bytes()) {
        Ok(request) => crate::run(request),
        Err(_) => 2,
    };
    // All request/runtime values have been dropped before unloading our code.
    FreeLibraryAndExitThread(MODULE.load(Ordering::Acquire), exit)
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
        return vec![format!("<enumeration failed: {}>", std::io::Error::last_os_error())];
    }
    let count = needed as usize / size_of::<HMODULE>();
    let mut modules: Vec<HMODULE> = vec![null_mut(); count];
    if EnumProcessModules(process, modules.as_mut_ptr(), needed, &mut needed) == 0 {
        return vec![format!("<enumeration failed: {}>", std::io::Error::last_os_error())];
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
