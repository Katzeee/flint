//! Attach: start a Bridge inside a running host by injecting the bootstrap.
//!
//! This is the injector that runs in flint's own process. It stages the host's
//! attach layout, has the host's platform turn its entry into a runtime plan, and
//! loads the bootstrap into the target, which executes that plan to start the Bridge.
//! Injection knows nothing about the backend; the caller waits for
//! [`attach_outcome`] to learn whether the injected side connected the Bridge.

use crate::{
    HostKind,
    bridge::{Attach, bridge},
    layout::stage,
};
use anyhow::Result;
use flint_contracts::{
    attach::{BootstrapRequest, read_report},
    config::StateDir,
    host_settings::HostSettings,
};

#[derive(Debug, thiserror::Error)]
#[error("attach is not implemented for host kind {0}")]
pub struct Unsupported(pub HostKind);

/// Only the Windows bootstrap can enter a host.
pub fn attachment(host: HostKind) -> Result<Attach, Unsupported> {
    let declaration = if cfg!(windows) { bridge(host).attach } else { None };
    declaration.ok_or(Unsupported(host))
}

/// Whether the most recent attach to `pid` connected its Bridge with the requested
/// settings, or why not; None while the injected side is still working.
pub fn attach_outcome(state: &StateDir, pid: u32) -> Option<Result<(), String>> {
    read_report(&state.attach_report_path(pid))
}

/// Inject the Bridge bootstrap into the host process `pid`.
///
/// Returns once the bootstrap has dispatched the plan; the injected side
/// reports the outcome through [`attach_outcome`].
impl Attach {
    pub fn inject(self, pid: u32, state: &StateDir, settings: HostSettings) -> Result<()> {
        let report_path = state.attach_report_path(pid);
        let root = stage(&state.attach_staging(), &self.layout)?;
        let plan = self.entry.plan(&root, settings);
        std::fs::create_dir_all(report_path.parent().expect("attach report has a parent directory"))?;
        // A previous attempt's outcome must not be mistaken for this one's.
        match std::fs::remove_file(&report_path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        os::inject(pid, state, &BootstrapRequest { plan, report_path })
    }
}

#[cfg(not(windows))]
mod os {
    pub fn inject(_pid: u32, _state: &super::StateDir, _request: &super::BootstrapRequest) -> anyhow::Result<()> {
        unreachable!("no host declares attach off Windows")
    }
}
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as os;
