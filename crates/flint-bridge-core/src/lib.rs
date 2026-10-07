//! Bridge connections and execution orchestration; hosts supply thread
//! scheduling, preparation, and invocation through the C ABI.

mod claim;
mod connection;
mod core;
mod execution;
mod ffi;
mod host;
mod settings;
mod state;

pub use core::BridgeCore;
pub use ffi::*;
pub use host::{FlintHost, Step, Ticket};

#[cfg(test)]
mod tests;
