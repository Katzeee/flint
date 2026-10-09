//! Run a plan's source on the host's CPython runtime.

use crate::os::{export, list_modules, module_with_export};
use anyhow::{Context, Result};
use flint_contracts::attach::CpythonPlan;
use std::{ffi::CString, mem::transmute, os::raw::c_char};

pub(crate) fn attach(plan: &CpythonPlan) -> Result<()> {
    unsafe {
        // Injecting this library just changed the module list, so an immediate
        // enumeration can miss the interpreter; retry until it is found and
        // initialized. The runtime may be a python3NN.dll or a statically
        // linked python.exe, so it is located by the export it provides.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let python = loop {
            if let Some(python) = module_with_export(c"Py_IsInitialized") {
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

        let source =
            CString::new(plan.source.as_str()).context("attach source contains a NUL byte")?;
        let gil = ensure();
        let code = run_string(source.as_ptr());
        release(gil);
        if code != 0 {
            return Err(anyhow::anyhow!(
                "the host CPython runtime rejected the attach source"
            ));
        }
        Ok(())
    }
}
