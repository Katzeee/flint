//! Attach: start a Bridge inside a running host by injecting the bootstrap.
//!
//! This is the injector that runs in flint's own process. It stages the host's
//! attach layout, has the host's platform turn its entry into a runtime plan, and
//! loads the bootstrap into the target, which executes that plan to start the Bridge.
//! Injection knows nothing about the backend; the caller confirms the Bridge
//! registered, or reads [`attach_error`] for why the injected side could not start it.

use crate::{
    HostKind,
    bridge::{Attach, bridge},
    layout::stage,
};
use anyhow::Result;
use flint_contracts::{attach::BootstrapRequest, config::StateDir, host_settings::HostSettings};
use strum::IntoEnumIterator;

#[derive(Debug, thiserror::Error)]
#[error("attach is not implemented for host kind {0}")]
pub struct Unsupported(pub HostKind);

/// Only the Windows bootstrap can enter a host.
fn declaration(host: HostKind) -> Option<Attach> {
    if cfg!(windows) { bridge(host).attach } else { None }
}

pub fn attachment(host: HostKind) -> Result<Attach, Unsupported> {
    declaration(host).ok_or(Unsupported(host))
}

pub fn attach_supported() -> bool {
    HostKind::iter().any(|host| declaration(host).is_some())
}

/// Why the most recent attach to `pid` could not start or re-point its Bridge,
/// once the injected side has reported it. Absent while it is still working or
/// after it succeeded.
pub fn attach_error(state: &StateDir, pid: u32) -> Option<String> {
    std::fs::read_to_string(state.attach_error_path(pid)).ok()
}

/// Inject the Bridge bootstrap into the host process `pid`.
///
/// Returns once the bootstrap has dispatched the plan; the Bridge connects
/// asynchronously and the caller confirms it through the backend.
impl Attach {
    pub fn inject(self, pid: u32, state: &StateDir, settings: HostSettings) -> Result<()> {
        let error_path = state.attach_error_path(pid);
        let root = stage(&state.attach_staging(), &self.layout)?;
        let plan = self.entry.plan(&root, settings);
        std::fs::create_dir_all(error_path.parent().expect("attach report has a parent directory"))?;
        // A previous attempt's outcome must not be mistaken for this one's.
        match std::fs::remove_file(&error_path) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        os::inject(pid, state, &BootstrapRequest { plan, error_path })
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
