//! Invoke a managed attach entry in the host's loaded Mono runtime.

use crate::os::{export, find_module, list_modules};
use anyhow::{Context, Result};
use std::ffi::{CStr, CString};
use std::mem::transmute;
use std::os::raw::{c_char, c_void};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::HMODULE;

use flint_contracts::attach::MonoPlan;

type MonoDomainFn = unsafe extern "C" fn(*mut c_void, *mut c_void);
type MonoFriendlyNameFn = unsafe extern "C" fn(*mut c_void) -> *const c_char;

struct DomainSearch<'a> {
    name: &'a str,
    target: *mut c_void,
    friendly_name: MonoFriendlyNameFn,
}

/// Find the scripting domain selected by the host.
unsafe extern "C" fn find_domain(domain: *mut c_void, user_data: *mut c_void) {
    let search = &mut *(user_data as *mut DomainSearch<'_>);
    if !search.target.is_null() {
        return;
    }
    let name = (search.friendly_name)(domain);
    if !name.is_null() && CStr::from_ptr(name).to_bytes() == search.name.as_bytes() {
        search.target = domain;
    }
}

/// Start the Bridge inside a managed host by loading the attach assembly and
/// invoking its entry on the runtime.
pub(crate) fn attach(plan: &MonoPlan) -> Result<()> {
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
        let result = attach_mono(mono, plan);
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
    ["mono-2.0-bdwgc.dll", "mono-2.0-sgen.dll"]
        .into_iter()
        .find_map(|name| find_module(name))
}

unsafe fn attach_mono(mono: HMODULE, plan: &MonoPlan) -> Result<()> {
    let get_root_domain: unsafe extern "C" fn() -> *mut c_void = transmute(export(mono, b"mono_get_root_domain\0")?);
    let thread_attach: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        transmute(export(mono, b"mono_thread_attach\0")?);
    let domain_set: unsafe extern "C" fn(*mut c_void, i32) -> i32 = transmute(export(mono, b"mono_domain_set\0")?);
    let domain_foreach: unsafe extern "C" fn(MonoDomainFn, *mut c_void) =
        transmute(export(mono, b"mono_domain_foreach\0")?);
    let friendly_name: MonoFriendlyNameFn = transmute(export(mono, b"mono_domain_get_friendly_name\0")?);
    let assembly_open: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void =
        transmute(export(mono, b"mono_domain_assembly_open\0")?);
    let get_image: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        transmute(export(mono, b"mono_assembly_get_image\0")?);
    let class_from_name: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> *mut c_void =
        transmute(export(mono, b"mono_class_from_name\0")?);
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

    // Prefer the host's scripting domain, falling back to the root domain.
    let mut search = DomainSearch {
        name: &plan.domain,
        target: null_mut(),
        friendly_name,
    };
    domain_foreach(find_domain, &mut search as *mut _ as *mut c_void);
    let domain = if search.target.is_null() { root } else { search.target };
    domain_set(domain, 0);
    thread_attach(domain);

    let payload = CString::new(plan.assembly.as_str()).context("assembly path has a NUL")?;
    let assembly = assembly_open(domain, payload.as_ptr());
    if assembly.is_null() {
        return Err(anyhow::anyhow!("could not open the attach assembly"));
    }
    let image = get_image(assembly);
    if image.is_null() {
        return Err(anyhow::anyhow!("attach assembly has no image"));
    }
    let namespace = CString::new(plan.namespace.as_str()).context("managed namespace has a NUL")?;
    let class_name = CString::new(plan.class.as_str()).context("managed class name has a NUL")?;
    let class = class_from_name(image, namespace.as_ptr(), class_name.as_ptr());
    if class.is_null() {
        return Err(anyhow::anyhow!(
            "{}.{} was not found in the assembly",
            plan.namespace,
            plan.class
        ));
    }
    let method_name = CString::new(plan.method.as_str()).context("managed method name has a NUL")?;
    let method = method_from_name(class, method_name.as_ptr(), 1);
    if method.is_null() {
        return Err(anyhow::anyhow!(
            "{}.{}.{}(string) was not found",
            plan.namespace,
            plan.class,
            plan.method
        ));
    }

    let argument = CString::new(plan.argument.as_str()).context("attach argument has a NUL")?;
    let managed = string_new(domain, argument.as_ptr());
    let mut arguments: [*mut c_void; 1] = [managed];
    let mut exception: *mut c_void = null_mut();
    let failure = runtime_invoke(method, null_mut(), arguments.as_mut_ptr(), &mut exception);
    if !exception.is_null() {
        return Err(anyhow::anyhow!("the managed attach entry threw an exception"));
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
