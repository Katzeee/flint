//! Flint's product API and executable entry point.

pub mod application;
mod cli;
mod desktop;

pub fn run() -> i32 {
    cli::run()
}
