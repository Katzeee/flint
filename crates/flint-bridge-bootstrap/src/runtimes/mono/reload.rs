//! Re-enter a managed adapter when its domain and assembly dependencies return.

use super::{Adapter, MonoDomainFn, detach_mono_thread};
use crate::os::{export, pin};
use anyhow::Result;
use flint_contracts::attach::MonoPlan;
use std::ffi::{CStr, c_char, c_void};
use std::mem::transmute;
use std::path::PathBuf;
use std::sync::{Mutex, mpsc};
use windows_sys::Win32::Foundation::HMODULE;

type Pointer = unsafe extern "C" fn() -> *mut c_void;
type DomainName = unsafe extern "C" fn(*mut c_void) -> *const c_char;
type DomainAction = unsafe extern "C" fn(*mut c_void);
type DomainSet = unsafe extern "C" fn(*mut c_void, i32) -> i32;

struct State {
    domain: usize,
    loaded: Vec<bool>,
    entered: bool,
    shutdown: bool,
}

struct Observer {
    module: usize,
    assembly: PathBuf,
    domain_name: String,
    entry: String,
    dependencies: Vec<String>,
    state: Mutex<State>,
    changed: mpsc::SyncSender<()>,
    current_domain: Pointer,
    assembly_name: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    name: DomainName,
    root: Pointer,
    attach: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    set_domain: DomainSet,
    push_domain: DomainAction,
    pop_domain: unsafe extern "C" fn(),
}

static OBSERVER: Mutex<Option<&'static Observer>> = Mutex::new(None);

pub(super) unsafe fn install(
    mono: HMODULE,
    plan: &MonoPlan,
    dependencies: &[String],
    domain: *mut c_void,
) -> Result<()> {
    let mut installed = OBSERVER.lock().unwrap();
    if installed.is_some() {
        return Ok(());
    }

    let create: unsafe extern "C" fn(*mut c_void) -> *mut c_void = transmute(export(mono, b"mono_profiler_create\0")?);
    let named: unsafe extern "C" fn(*mut c_void, unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char)) =
        transmute(export(mono, b"mono_profiler_set_domain_name_callback\0")?);
    let loaded: unsafe extern "C" fn(*mut c_void, MonoDomainFn) =
        transmute(export(mono, b"mono_profiler_set_assembly_loaded_callback\0")?);
    let unloading: unsafe extern "C" fn(*mut c_void, MonoDomainFn) =
        transmute(export(mono, b"mono_profiler_set_domain_unloading_callback\0")?);
    let shutdown: unsafe extern "C" fn(*mut c_void, DomainAction) =
        transmute(export(mono, b"mono_profiler_set_runtime_shutdown_begin_callback\0")?);
    let (changed, receiver) = mpsc::sync_channel(1);
    let observer = Box::new(Observer {
        module: mono as usize,
        assembly: plan.assembly.clone(),
        domain_name: plan.domain.clone(),
        entry: plan.entry.clone(),
        dependencies: dependencies.to_vec(),
        state: Mutex::new(State {
            domain: domain as usize,
            loaded: vec![false; dependencies.len()],
            // The explicit attach owns initialization in the current domain.
            entered: true,
            shutdown: false,
        }),
        changed,
        current_domain: transmute::<*const (), Pointer>(export(mono, b"mono_domain_get\0")?),
        assembly_name: transmute::<*const (), unsafe extern "C" fn(*mut c_void) -> *mut c_void>(export(
            mono,
            b"mono_assembly_get_name\0",
        )?),
        name: transmute::<*const (), DomainName>(export(mono, b"mono_assembly_name_get_name\0")?),
        root: transmute::<*const (), Pointer>(export(mono, b"mono_get_root_domain\0")?),
        attach: transmute::<*const (), unsafe extern "C" fn(*mut c_void) -> *mut c_void>(export(
            mono,
            b"mono_thread_attach\0",
        )?),
        set_domain: transmute::<*const (), DomainSet>(export(mono, b"mono_domain_set\0")?),
        push_domain: transmute::<*const (), DomainAction>(export(mono, b"mono_thread_push_appdomain_ref\0")?),
        pop_domain: transmute::<*const (), unsafe extern "C" fn()>(export(mono, b"mono_thread_pop_appdomain_ref\0")?),
    });
    pin()?;
    // Mono retains the callback context until runtime shutdown.
    let observer: &'static Observer = Box::leak(observer);
    let (started, ready) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("flint-mono-reload".into())
        .spawn(move || {
            // Join the runtime before publishing callbacks. Keep this worker in
            // the root domain between notifications so shutdown can account for it.
            unsafe {
                (observer.attach)((observer.root)());
            }
            let _ = started.send(());
            while receiver.recv().is_ok() {
                unsafe {
                    if !observer.restore() {
                        break;
                    }
                }
            }
            unsafe {
                detach_mono_thread(observer.module as HMODULE);
            }
        })?;
    ready.recv()?;
    let context = observer as *const Observer as *mut c_void;
    let profiler = create(context);
    named(profiler, domain_named);
    loaded(profiler, assembly_loaded);
    unloading(profiler, domain_unloading);
    shutdown(profiler, runtime_shutdown);
    *installed = Some(observer);
    Ok(())
}

impl Observer {
    unsafe fn loaded(&self, domain: *mut c_void, assembly: *mut c_void) {
        let mut state = self.state.lock().unwrap();
        if state.domain != domain as usize || state.entered {
            return;
        }
        let name = CStr::from_ptr((self.name)((self.assembly_name)(assembly))).to_bytes();
        for (index, dependency) in self.dependencies.iter().enumerate() {
            if name == dependency.as_bytes() {
                state.loaded[index] = true;
            }
        }
        if state.loaded.iter().all(|loaded| *loaded) {
            let _ = self.changed.try_send(());
        }
    }

    unsafe fn restore(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.shutdown {
            return false;
        }
        if state.domain == 0 || state.entered || !state.loaded.iter().all(|loaded| *loaded) {
            return true;
        }
        let mono = self.module as HMODULE;
        let root = (self.root)();
        let domain = state.domain as *mut c_void;
        // Pair the native lifetime guard with Mono's domain reference. Unload
        // cannot free the pointer between selecting it and recording this ref.
        (self.push_domain)(domain);
        state.entered = true;
        drop(state);
        if (self.set_domain)(domain, 0) != 0 {
            let restored = Adapter::load(mono, &self.assembly, domain)
                .and_then(|adapter| adapter.call(mono, domain, c"Restore", &self.entry));
            if let Err(error) = restored {
                eprintln!("Flint managed reload failed: {error:#}");
            }
        }
        (self.set_domain)(root, 0);
        (self.pop_domain)();
        true
    }
}

unsafe extern "C" fn domain_named(context: *mut c_void, domain: *mut c_void, name: *const c_char) {
    let observer = &*(context as *const Observer);
    if name.is_null() || CStr::from_ptr(name).to_bytes() != observer.domain_name.as_bytes() {
        return;
    }
    let mut state = observer.state.lock().unwrap();
    state.domain = domain as usize;
    state.loaded.fill(false);
    state.entered = false;
}

unsafe extern "C" fn assembly_loaded(context: *mut c_void, assembly: *mut c_void) {
    let observer = &*(context as *const Observer);
    observer.loaded((observer.current_domain)(), assembly);
}

unsafe extern "C" fn domain_unloading(context: *mut c_void, domain: *mut c_void) {
    let observer = &*(context as *const Observer);
    let mut state = observer.state.lock().unwrap();
    if state.domain == domain as usize {
        // Mono can reuse both a domain's address and its numeric ID.
        state.domain = 0;
    }
}

unsafe extern "C" fn runtime_shutdown(context: *mut c_void) {
    let observer = &*(context as *const Observer);
    observer.state.lock().unwrap().shutdown = true;
    let _ = observer.changed.try_send(());
}
