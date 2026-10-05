//! Bridge connections and execution transport; host adapters own code execution.

mod claim;
mod connection;
mod core;
mod execution;
mod ffi;
mod settings;
mod state;

pub use core::BridgeCore;
pub use ffi::*;

#[cfg(test)]
mod tests;
