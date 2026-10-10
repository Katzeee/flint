//! C callers own handle lifetime. Returned strings must be released through this library.

use crate::{
    BridgeCore,
    core::CreationError,
    execution_binding::{ExecutionBinding, OwnedExecutionBinding},
    execution_coordinator::Step,
    settings::{ApplyResult, BridgeOptions, HostSettings},
};
use anyhow::Context;
use std::{
    ffi::{CStr, CString, c_char},
    ptr,
};

unsafe fn input(value: *const c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        CStr::from_ptr(value).to_str().ok().map(str::to_owned)
    }
}

/// A blank string from a host carries no value.
unsafe fn present(value: *const c_char) -> Option<String> {
    input(value).filter(|value| !value.trim().is_empty())
}

unsafe fn options(config_json: *const c_char) -> anyhow::Result<BridgeOptions> {
    anyhow::ensure!(!config_json.is_null(), "it is null");
    let text = CStr::from_ptr(config_json).to_str().context("it is not UTF-8")?;
    Ok(serde_json::from_str(text)?)
}

/// Creation categories in the C ABI, independent of Rust's source-error variants.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum CreationErrorKind {
    InvalidConfiguration = 1,
    Claimed = 2,
    System = 3,
}

impl From<&CreationError> for CreationErrorKind {
    fn from(error: &CreationError) -> Self {
        match error {
            CreationError::InvalidConfiguration(_) => Self::InvalidConfiguration,
            CreationError::Claimed(_) => Self::Claimed,
            CreationError::System(_) => Self::System,
        }
    }
}

#[no_mangle]
pub extern "C" fn flint_bridge_abi_version() -> u32 {
    9
}

/// Returns null on failure and sets `error_kind` to a nonzero
/// [`CreationErrorKind`] code and `error_message` to its text. Either output may be
/// null. The execution binding is released on failure.
///
/// # Safety
/// Non-null input pointers must address readable, aligned values, with `config_json` NUL-terminated.
/// Non-null output pointers must be aligned, writable, and exclusively borrowed for the call.
/// Binding callbacks and their context must remain valid until `release` completes and must
/// follow the execution binding contract in `include/flint_bridge_core.h`.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_create(
    config_json: *const c_char,
    execution_binding: *const ExecutionBinding,
    error_kind: *mut u32,
    error_message: *mut *mut c_char,
) -> *mut BridgeCore {
    if !error_kind.is_null() {
        *error_kind = 0;
    }
    if !error_message.is_null() {
        *error_message = ptr::null_mut();
    }
    let result = (|| {
        let Some(execution_binding) = execution_binding.as_ref().copied().map(OwnedExecutionBinding::new) else {
            return Err(CreationError::InvalidConfiguration(anyhow::anyhow!(
                "the execution binding is null"
            )));
        };
        let options = options(config_json).map_err(CreationError::InvalidConfiguration)?;
        BridgeCore::new(options, execution_binding)
    })();
    match result {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(error) => {
            if !error_kind.is_null() {
                *error_kind = CreationErrorKind::from(&error) as u32;
            }
            if !error_message.is_null() {
                let message = format!("{:#}", anyhow::Error::new(error));
                *error_message = CString::new(message.replace('\0', "\\0"))
                    .expect("error text has no NUL bytes")
                    .into_raw();
            }
            ptr::null_mut()
        }
    }
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_connected(core: *const BridgeCore) -> bool {
    core.as_ref().is_some_and(BridgeCore::connected)
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_busy(core: *const BridgeCore) -> bool {
    core.as_ref().is_some_and(BridgeCore::busy)
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_stopped(core: *const BridgeCore) -> bool {
    core.as_ref().is_none_or(BridgeCore::stopped)
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_instance_id(core: *const BridgeCore) -> *mut c_char {
    let Some(core) = core.as_ref() else {
        return ptr::null_mut();
    };
    CString::new(core.instance_id()).unwrap().into_raw()
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_reconnect(core: *const BridgeCore) -> bool {
    core.as_ref().is_some_and(BridgeCore::reconnect)
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_status_json(core: *const BridgeCore) -> *mut c_char {
    let Some(core) = core.as_ref() else {
        return ptr::null_mut();
    };
    CString::new(core.status_json()).unwrap().into_raw()
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
/// A non-null `settings_json` must point to a readable NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_apply_settings(core: *const BridgeCore, settings_json: *const c_char) -> u32 {
    let Some(core) = core.as_ref() else {
        return ApplyResult::Invalid as u32;
    };
    let Some(settings) = input(settings_json).and_then(|text| serde_json::from_str::<HostSettings>(&text).ok()) else {
        return ApplyResult::Invalid as u32;
    };
    core.apply_settings(settings) as u32
}

/// # Safety
/// `core` must be null or a live handle from `flint_bridge_create` for the entire call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_stop(core: *const BridgeCore) -> bool {
    core.as_ref().is_none_or(BridgeCore::stop)
}

/// # Safety
/// `core` must be null or an owned handle from `flint_bridge_create` that has not been destroyed.
/// No other call may access that handle during or after destruction.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_destroy(core: *mut BridgeCore) {
    if !core.is_null() {
        drop(Box::from_raw(core));
    }
}

#[no_mangle]
pub extern "C" fn flint_step_run(step: usize) {
    Step::run(step);
}

/// # Safety
/// Each non-null buffer must be readable for its specified length for the entire call.
/// The lengths must not exceed `isize::MAX`; callers must not mutate the buffers concurrently.
#[no_mangle]
pub unsafe extern "C" fn flint_step_output(
    step: usize,
    stdout: *const u8,
    stdout_len: usize,
    stderr: *const u8,
    stderr_len: usize,
) -> bool {
    let (Some(stdout), Some(stderr)) = (text(stdout, stdout_len), text(stderr, stderr_len)) else {
        return false;
    };
    Step::output(step, stdout, stderr)
}

unsafe fn text<'a>(data: *const u8, len: usize) -> Option<&'a str> {
    if len == 0 {
        return Some("");
    }
    if data.is_null() {
        return None;
    }
    std::str::from_utf8(std::slice::from_raw_parts(data, len)).ok()
}

#[no_mangle]
pub extern "C" fn flint_step_succeed(step: usize, result_id: usize) {
    Step::succeed(step, result_id);
}

/// # Safety
/// Each non-null string pointer must address a readable NUL-terminated string for the call.
#[no_mangle]
pub unsafe extern "C" fn flint_step_fail(
    step: usize,
    code: *const c_char,
    message: *const c_char,
    traceback: *const c_char,
) {
    Step::fail(step, present(code), present(message), input(traceback));
}

/// # Safety
/// `value` must be null or an unmodified string returned by this library that has not been freed.
/// No other call may access that string during or after this call.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests;
