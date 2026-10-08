//! The host execution ABI and its release ownership.
use crate::execution_coordinator::{Step, Ticket};
use std::ffi::c_char;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExecutionBinding {
    pub context: usize,
    pub post: unsafe extern "C" fn(usize, *mut Ticket) -> bool,
    pub prepare: unsafe extern "C" fn(usize, *const c_char, *mut Step),
    pub run: unsafe extern "C" fn(usize, usize, *mut Step),
    pub discard: unsafe extern "C" fn(usize, usize),
    pub release: unsafe extern "C" fn(usize),
}

/// Owns the execution binding and releases it when its last caller is done.
pub(crate) struct OwnedExecutionBinding(pub(crate) ExecutionBinding);

impl OwnedExecutionBinding {
    pub(crate) fn new(execution_binding: ExecutionBinding) -> Self {
        Self(execution_binding)
    }
}

impl Drop for OwnedExecutionBinding {
    fn drop(&mut self) {
        unsafe { (self.0.release)(self.0.context) }
    }
}
