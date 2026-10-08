//! The host execution ABI and its release ownership.
use std::ffi::c_char;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct ExecutionBinding {
    pub context: usize,
    pub post: unsafe extern "C" fn(context: usize, step: usize) -> bool,
    pub prepare: unsafe extern "C" fn(context: usize, request: *const c_char, step: usize),
    pub run: unsafe extern "C" fn(context: usize, result_id: usize, step: usize),
    pub discard: unsafe extern "C" fn(context: usize, result_id: usize),
    pub release: unsafe extern "C" fn(context: usize),
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
