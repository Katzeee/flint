//! Bridge connections and execution orchestration; hosts supply thread
//! scheduling, preparation, and invocation through the C ABI.

mod claim;
mod connection;
mod core;
mod execution;
mod execution_binding;
mod execution_coordinator;
mod ffi;
mod settings;
mod state;

pub use core::BridgeCore;
pub use execution_binding::ExecutionBinding;
pub use execution_coordinator::{Step, Ticket};
pub use ffi::*;

#[cfg(test)]
mod tests;
