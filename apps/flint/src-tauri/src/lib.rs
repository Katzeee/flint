//! Flint's product API and executable entry point.

pub mod application;
#[cfg(feature = "desktop")]
mod cli;
mod desktop;

#[cfg(feature = "bindings")]
pub use desktop::export_desktop_bindings;

#[cfg(feature = "desktop")]
pub fn run() -> i32 {
    cli::run()
}
