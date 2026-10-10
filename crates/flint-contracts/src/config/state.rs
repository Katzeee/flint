//! The layout of a backend's state directory and the runtime resources within it.
use crate::{attach::BootstrapRequest, lock::FileLock};
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct StateDir(PathBuf);

impl StateDir {
    pub fn new(root: PathBuf) -> Self {
        Self(root)
    }

    pub fn root(&self) -> &Path {
        &self.0
    }

    pub fn workflows(&self) -> PathBuf {
        self.0.join("workflows")
    }

    pub fn runtime(&self) -> Runtime {
        Runtime(self.0.join("runtime"))
    }

    pub fn attach_handoff(&self, pid: u32) -> BootstrapRequest {
        let directory = self.runtime().0.join("attach");
        BootstrapRequest {
            plan_path: directory.join(format!("{pid}.json")),
            error_path: directory.join(format!("{pid}.error")),
        }
    }

    pub fn attach_staging(&self) -> PathBuf {
        self.0.join("cache").join("attach")
    }
}

/// Files the backend and its control clients coordinate the backend's lifetime through.
#[derive(Debug, Clone)]
pub struct Runtime(PathBuf);

impl Runtime {
    pub fn log_path(&self) -> PathBuf {
        self.0.join("backend.log")
    }

    /// Held by the backend for its entire serving lifetime.
    pub fn claim_backend(&self) -> io::Result<Option<FileLock>> {
        FileLock::try_acquire(&self.0.join("backend.lock"))
    }

    /// Held by control clients throughout a start, stop, or restart operation.
    pub fn try_lifecycle(&self) -> io::Result<Option<FileLock>> {
        FileLock::try_acquire(&self.0.join("lifecycle.lock"))
    }

    /// Briefly tries the backend lease. Call while holding the lifecycle lock
    /// so the probe does not compete with another client's backend startup.
    pub fn backend_running(&self) -> io::Result<bool> {
        Ok(self.claim_backend()?.is_none())
    }
}
