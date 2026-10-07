//! C callers own handle lifetime. Returned strings must be released through this library.

use crate::{
    core::CreationError,
    host::{FlintHost, OwnedHost, Step, Ticket},
    settings::{ApplyResult, BridgeOptions, BridgeSettings},
    BridgeCore,
};
use std::{
    ffi::{c_char, CStr, CString},
    ptr,
};

unsafe fn input(value: *const c_char) -> Option<String> {
    if value.is_null() {
        None
    } else {
        CStr::from_ptr(value).to_str().ok().map(str::to_owned)
    }
}

#[no_mangle]
pub extern "C" fn flint_bridge_abi_version() -> u32 {
    6
}

/// Returns null on failure and sets `error_kind` to a nonzero
/// [`CreationError`] code and `error_message` to its text. Either output may be
/// null. The host registration is released on failure.
#[no_mangle]
pub unsafe extern "C" fn flint_bridge_create(
    config_json: *const c_char,
    host: *const FlintHost,
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
        let Some(host) = host.as_ref().copied().map(OwnedHost::new) else {
            return Err(CreationError::InvalidConfiguration(
                "the execution host is null".into(),
            ));
        };
        if config_json.is_null() {
            return Err(CreationError::InvalidConfiguration("it is null".into()));
        }
        let text = CStr::from_ptr(config_json).to_str().map_err(|error| {
            CreationError::InvalidConfiguration(format!("it is not UTF-8: {error}"))
        })?;
        let options = serde_json::from_str::<BridgeOptions>(text)
            .map_err(|error| CreationError::InvalidConfiguration(error.to_string()))?;
        BridgeCore::new(options, host)
    })();
    match result {
        Ok(core) => Box::into_raw(Box::new(core)),
        Err(error) => {
            if !error_kind.is_null() {
                *error_kind = error.code();
            }
            if !error_message.is_null() {
                *error_message = CString::new(error.message().replace('\0', "\\0"))
                    .expect("error text has no NUL bytes")
                    .into_raw();
            }
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_connected(core: *const BridgeCore) -> bool {
    core.as_ref().is_some_and(BridgeCore::connected)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_busy(core: *const BridgeCore) -> bool {
    core.as_ref().is_some_and(BridgeCore::busy)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_stopped(core: *const BridgeCore) -> bool {
    core.as_ref().is_none_or(BridgeCore::stopped)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_instance_id(core: *const BridgeCore) -> *mut c_char {
    let Some(core) = core.as_ref() else {
        return ptr::null_mut();
    };
    CString::new(core.instance_id()).unwrap().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_reconnect(core: *const BridgeCore) -> bool {
    core.as_ref().is_some_and(BridgeCore::reconnect)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_status_json(core: *const BridgeCore) -> *mut c_char {
    let Some(core) = core.as_ref() else {
        return ptr::null_mut();
    };
    CString::new(core.status_json()).unwrap().into_raw()
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_apply_settings(
    core: *const BridgeCore,
    settings_json: *const c_char,
) -> u32 {
    let Some(core) = core.as_ref() else {
        return ApplyResult::Invalid as u32;
    };
    let Some(settings) =
        input(settings_json).and_then(|text| serde_json::from_str::<BridgeSettings>(&text).ok())
    else {
        return ApplyResult::Invalid as u32;
    };
    core.apply_settings(settings) as u32
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_stop(core: *const BridgeCore) -> bool {
    core.as_ref().is_none_or(BridgeCore::stop)
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_destroy(core: *mut BridgeCore) {
    if !core.is_null() {
        drop(Box::from_raw(core));
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_ticket_run(ticket: *mut Ticket) {
    if !ticket.is_null() {
        Box::from_raw(ticket).run();
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_ticket_drop(ticket: *mut Ticket) {
    if !ticket.is_null() {
        Box::from_raw(ticket).drop_unrun();
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_step_output(
    step: *const Step,
    stdout: *const u8,
    stdout_len: usize,
    stderr: *const u8,
    stderr_len: usize,
) -> bool {
    let (Some(step), Some(stdout), Some(stderr)) = (
        step.as_ref(),
        text(stdout, stdout_len),
        text(stderr, stderr_len),
    ) else {
        return false;
    };
    step.output(stdout, stderr)
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
pub unsafe extern "C" fn flint_step_succeed(step: *mut Step, prepared: usize) {
    if !step.is_null() {
        Box::from_raw(step).succeed(prepared);
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_step_fail(
    step: *mut Step,
    traceback: *const c_char,
    error: *const c_char,
) {
    if !step.is_null() {
        Box::from_raw(step).fail(input(traceback), input(error));
    }
}

#[no_mangle]
pub unsafe extern "C" fn flint_bridge_string_free(value: *mut c_char) {
    if !value.is_null() {
        drop(CString::from_raw(value));
    }
}

#[cfg(test)]
mod tests;
