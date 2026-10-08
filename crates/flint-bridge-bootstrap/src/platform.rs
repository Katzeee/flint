//! Windows entry point and CPython driver for the injected bootstrap.

use std::ffi::{CStr, CString};
use std::mem::{size_of, transmute};
use std::os::raw::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::System::LibraryLoader::{
    FreeLibraryAndExitThread, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::ProcessStatus::{EnumProcessModules, GetModuleBaseNameW};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::Win32::System::Threading::{CreateThread, GetCurrentProcess, GetCurrentProcessId};

use crate::AttachConfig;
use anyhow::{Context, Result};

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

/// Start the Bridge inside a CPython host by running a short bootstrap on the
/// interpreter, which loads the Bridge package and calls `flint_bridge.attach`.
/// That call finishes on its own thread and reports failure to `error_path`.
pub fn attach_cpython(config: &AttachConfig, error_path: &std::path::Path) -> Result<()> {
    unsafe {
        // Injecting this library just changed the module list, so an immediate
        // enumeration can miss the interpreter; retry until it is found and
        // initialized. The runtime may be a python3NN.dll or a statically
        // linked python.exe, so it is located by the export it provides.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let python = loop {
            if let Some(python) = find_python_runtime() {
                let is_initialized: unsafe extern "C" fn() -> i32 =
                    transmute(export(python, b"Py_IsInitialized\0")?);
                if is_initialized() != 0 {
                    break python;
                }
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "no initialized CPython runtime is loaded in the host; modules seen: {}",
                    list_modules().join(", ")
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let ensure: unsafe extern "C" fn() -> i32 =
            transmute(export(python, b"PyGILState_Ensure\0")?);
        let release: unsafe extern "C" fn(i32) =
            transmute(export(python, b"PyGILState_Release\0")?);
        let run_string: unsafe extern "C" fn(*const c_char) -> i32 =
            transmute(export(python, b"PyRun_SimpleString\0")?);

        let source = CString::new(crate::python_bootstrap(config, error_path))
            .context("attach bootstrap contains a NUL byte")?;
        // The bootstrap only schedules a daemon thread, so it returns promptly
        // and does not hold the GIL while the connection is established.
        let gil = ensure();
        let code = run_string(source.as_ptr());
        release(gil);
        if code != 0 {
            return Err(anyhow::anyhow!(
                "the host CPython runtime rejected the attach bootstrap"
            ));
        }
        Ok(())
    }
}

/// Resolve an export by null-terminated name, returning its address.
unsafe fn export(module: HMODULE, name: &[u8]) -> Result<*const ()> {
    match GetProcAddress(module, name.as_ptr()) {
        Some(address) => Ok(address as *const ()),
        None => Err(anyhow::anyhow!(
            "host runtime is missing {}",
            String::from_utf8_lossy(&name[..name.len() - 1])
        )),
    }
}

/// Find the module that provides the CPython C API, identified by the export it
/// carries. This is the interpreter whether it is a `python3NN.dll` or a
/// statically linked `python.exe`, and works regardless of the module's name.
unsafe fn find_python_runtime() -> Option<HMODULE> {
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
    modules.into_iter().find(|&module| {
        !module.is_null() && GetProcAddress(module, c"Py_IsInitialized".as_ptr().cast()).is_some()
    })
}

/// The base names of every module currently loaded in this process, for
/// diagnostics when the interpreter cannot be found.
unsafe fn list_modules() -> Vec<String> {
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

// --- Managed runtime (.NET) attach ---------------------------------------------
//
// The `dotnet` runtime tag is resolved in-process: Mono today, CoreCLR later.
// This keeps the injector and config format stable when CoreCLR support is added
// — only a branch here changes. The injection mechanism above is shared.

type MonoDomainFn = unsafe extern "C" fn(*mut c_void, *mut c_void);
type MonoFriendlyNameFn = unsafe extern "C" fn(*mut c_void) -> *const c_char;

struct DomainSearch {
    target: *mut c_void,
    friendly_name: MonoFriendlyNameFn,
}

/// Callback for `mono_domain_foreach`: record the "Unity Child Domain", where
/// Unity runs editor scripts, so the managed entry loads into it.
unsafe extern "C" fn find_child_domain(domain: *mut c_void, user_data: *mut c_void) {
    let search = &mut *(user_data as *mut DomainSearch);
    if !search.target.is_null() {
        return;
    }
    let name = (search.friendly_name)(domain);
    if !name.is_null() && CStr::from_ptr(name).to_bytes() == b"Unity Child Domain" {
        search.target = domain;
    }
}

/// Start the Bridge inside a managed host by loading the attach assembly and
/// invoking its entry on the runtime.
pub fn attach_dotnet(config: &AttachConfig) -> Result<()> {
    unsafe {
        // Injection just changed the module list; give the loader a moment.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mono = loop {
            if let Some(mono) = find_mono() {
                break mono;
            }
            if std::time::Instant::now() >= deadline {
                return Err(anyhow::anyhow!(
                    "no supported managed runtime is loaded (Mono expected; CoreCLR attach is \
                     not yet implemented and IL2CPP cannot be attached); modules seen: {}",
                    list_modules().join(", ")
                ));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        };
        let result = attach_mono(mono, config);
        // The worker exits next, so leave the runtime it joined.
        detach_mono_thread(mono);
        result
    }
}

unsafe fn detach_mono_thread(mono: HMODULE) {
    let (Ok(current), Ok(detach)) = (
        export(mono, b"mono_thread_current\0"),
        export(mono, b"mono_thread_detach\0"),
    ) else {
        return;
    };
    let current: unsafe extern "C" fn() -> *mut c_void = transmute(current);
    let detach: unsafe extern "C" fn(*mut c_void) = transmute(detach);
    let thread = current();
    if !thread.is_null() {
        detach(thread);
    }
}

/// The loaded Mono runtime module, if any (either garbage collector flavor).
unsafe fn find_mono() -> Option<HMODULE> {
    for name in ["mono-2.0-bdwgc.dll", "mono-2.0-sgen.dll"] {
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let module = GetModuleHandleW(wide.as_ptr());
        if !module.is_null() {
            return Some(module);
        }
    }
    None
}

unsafe fn attach_mono(mono: HMODULE, config: &AttachConfig) -> Result<()> {
    let get_root_domain: unsafe extern "C" fn() -> *mut c_void =
        transmute(export(mono, b"mono_get_root_domain\0")?);
    let thread_attach: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        transmute(export(mono, b"mono_thread_attach\0")?);
    let domain_set: unsafe extern "C" fn(*mut c_void, i32) -> i32 =
        transmute(export(mono, b"mono_domain_set\0")?);
    let domain_foreach: unsafe extern "C" fn(MonoDomainFn, *mut c_void) =
        transmute(export(mono, b"mono_domain_foreach\0")?);
    let friendly_name: MonoFriendlyNameFn =
        transmute(export(mono, b"mono_domain_get_friendly_name\0")?);
    let assembly_open: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
        transmute(export(mono, b"mono_domain_assembly_open\0")?);
    let get_image: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        transmute(export(mono, b"mono_assembly_get_image\0")?);
    let class_from_name: unsafe extern "C" fn(
        *mut c_void,
        *const c_char,
        *const c_char,
    ) -> *mut c_void = transmute(export(mono, b"mono_class_from_name\0")?);
    let method_from_name: unsafe extern "C" fn(*mut c_void, *const c_char, i32) -> *mut c_void =
        transmute(export(mono, b"mono_class_get_method_from_name\0")?);
    let string_new: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
        transmute(export(mono, b"mono_string_new\0")?);
    let string_to_utf8: unsafe extern "C" fn(*mut c_void) -> *mut c_char =
        transmute(export(mono, b"mono_string_to_utf8\0")?);
    let mono_free: unsafe extern "C" fn(*mut c_void) = transmute(export(mono, b"mono_free\0")?);
    let runtime_invoke: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        *mut *mut c_void,
        *mut *mut c_void,
    ) -> *mut c_void = transmute(export(mono, b"mono_runtime_invoke\0")?);

    let root = get_root_domain();
    if root.is_null() {
        return Err(anyhow::anyhow!("Mono root domain is unavailable"));
    }
    thread_attach(root);

    // Prefer Unity's scripts domain; fall back to root for other Mono hosts.
    let mut search = DomainSearch {
        target: null_mut(),
        friendly_name,
    };
    domain_foreach(find_child_domain, &mut search as *mut _ as *mut c_void);
    let domain = if search.target.is_null() {
        root
    } else {
        search.target
    };
    domain_set(domain, 0);
    thread_attach(domain);

    let payload = CString::new(config.payload.as_str()).context("assembly path has a NUL")?;
    let assembly = assembly_open(domain, payload.as_ptr());
    if assembly.is_null() {
        return Err(anyhow::anyhow!("could not open the attach assembly"));
    }
    let image = get_image(assembly);
    if image.is_null() {
        return Err(anyhow::anyhow!("attach assembly has no image"));
    }
    let namespace = CString::new("Flint.Unity").unwrap();
    let class_name = CString::new("Attach").unwrap();
    let class = class_from_name(image, namespace.as_ptr(), class_name.as_ptr());
    if class.is_null() {
        return Err(anyhow::anyhow!(
            "Flint.Unity.Attach was not found in the assembly"
        ));
    }
    let method_name = CString::new("Initialize").unwrap();
    let method = method_from_name(class, method_name.as_ptr(), 1);
    if method.is_null() {
        return Err(anyhow::anyhow!(
            "Flint.Unity.Attach.Initialize(string) was not found"
        ));
    }

    // The entry parses newline-delimited fields: address, port, name, core path.
    let argument = format!(
        "{}\n{}\n{}\n{}",
        config.address,
        config.port,
        config.name,
        config.core_path.as_deref().unwrap_or("")
    );
    let argument = CString::new(argument).context("attach argument has a NUL")?;
    let managed = string_new(domain, argument.as_ptr());
    let mut arguments: [*mut c_void; 1] = [managed];
    let mut exception: *mut c_void = null_mut();
    let failure = runtime_invoke(method, null_mut(), arguments.as_mut_ptr(), &mut exception);
    if !exception.is_null() {
        return Err(anyhow::anyhow!(
            "the managed attach entry threw an exception"
        ));
    }
    // The entry returns null on success, or why it could not start the Bridge.
    if !failure.is_null() {
        let utf8 = string_to_utf8(failure);
        let message = CStr::from_ptr(utf8).to_string_lossy().into_owned();
        mono_free(utf8.cast());
        return Err(anyhow::anyhow!(message));
    }
    Ok(())
}
