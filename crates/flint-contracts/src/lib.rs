//! What separately built Flint binaries exchange.

pub mod attach;
pub mod config;
pub mod host_settings;
pub mod instance;
pub mod lock;
#[cfg(feature = "protocol")]
pub mod protocol;
