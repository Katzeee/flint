//! Enter Flint's Python attach adapter on the host's CPython runtime.

use crate::os::{export, list_modules, module_with_export};
use anyhow::{Context, Result};
use flint_contracts::attach::CpythonPlan;
use std::{ffi::CString, mem::transmute, os::raw::c_char, path::Path};

pub(crate) fn attach(plan: &CpythonPlan, error_path: &Path) -> Result<()> {
    unsafe {
        // Injecting this library just changed the module list, so an immediate
        // enumeration can miss the interpreter; retry until it is found and
        // initialized. The runtime may be a python3NN.dll or a statically
        // linked python.exe, so it is located by the export it provides.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let python = loop {
            if let Some(python) = module_with_export(c"Py_IsInitialized") {
                let is_initialized: unsafe extern "C" fn() -> i32 = transmute(export(python, b"Py_IsInitialized\0")?);
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
        let ensure: unsafe extern "C" fn() -> i32 = transmute(export(python, b"PyGILState_Ensure\0")?);
        let release: unsafe extern "C" fn(i32) = transmute(export(python, b"PyGILState_Release\0")?);
        let run_string: unsafe extern "C" fn(*const c_char) -> i32 =
            transmute(export(python, b"PyRun_SimpleString\0")?);

        let source = CString::new(source(plan, error_path)?).context("attach source contains a NUL byte")?;
        let gil = ensure();
        let code = run_string(source.as_ptr());
        release(gil);
        if code != 0 {
            return Err(anyhow::anyhow!("the host CPython runtime rejected the attach source"));
        }
        Ok(())
    }
}

/// Encode at the Python boundary so paths and names remain data in the plan.
fn source(plan: &CpythonPlan, error_path: &Path) -> Result<String> {
    Ok(format!(
        "import sys\n\
         if {root} not in sys.path:\n    sys.path.insert(0, {root})\n\
         from flint_bridge.attach import start\n\
         start({plan}, {error_path})\n",
        root = serde_json::to_string(&plan.import_root)?,
        plan = serde_json::to_string(&serde_json::to_string(plan)?)?,
        error_path = serde_json::to_string(error_path)?,
    ))
}

#[cfg(test)]
mod tests;
