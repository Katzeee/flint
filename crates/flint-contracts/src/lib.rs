//! What separately built Flint binaries exchange.

pub mod attach;
pub mod config;
pub mod lock;
#[cfg(feature = "protocol")]
pub mod protocol;
