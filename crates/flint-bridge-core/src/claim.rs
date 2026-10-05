//! The OS lock enforces one Bridge across independently loaded library copies
//! and language runtimes in the same process. A Rust static cannot do that.

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{fmt, fs, io, path::PathBuf};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ClaimOwner {
    pub(crate) host: String,
    pub(crate) runtime_version: String,
    pub(crate) bridge_version: String,
}

impl fmt::Display for ClaimOwner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "host={}, runtime_version={}, bridge_version={}",
            self.host, self.runtime_version, self.bridge_version
        )
    }
}

pub(crate) enum ClaimOutcome {
    Acquired(ProcessClaim),
    Occupied(Option<ClaimOwner>),
}

/// Holds the process's Bridge claim. Dropping it releases the lock.
pub(crate) struct ProcessClaim {
    lock: fs::File,
}

fn directory() -> PathBuf {
    std::env::temp_dir().join("flint-bridge").join("claims")
}

pub(crate) fn acquire(owner: &ClaimOwner) -> io::Result<ClaimOutcome> {
    acquire_scope(&std::process::id().to_string(), owner)
}

#[cfg(test)]
pub(crate) fn acquire_for_test(scope: &str, owner: &ClaimOwner) -> io::Result<ClaimOutcome> {
    acquire_scope(scope, owner)
}

fn acquire_scope(scope: &str, owner: &ClaimOwner) -> io::Result<ClaimOutcome> {
    let directory = directory();
    fs::create_dir_all(&directory)?;
    let lock_path = directory.join(format!("{scope}.lock"));
    let owner_path = directory.join(format!("{scope}.owner"));
    let lock = fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)?;
    match lock.try_lock_exclusive() {
        Ok(()) => {
            // The descriptor is a diagnostic aid; failing to record it must not
            // forfeit an otherwise valid claim.
            if let Ok(record) = serde_json::to_vec(owner) {
                let _ = fs::write(&owner_path, record);
            }
            Ok(ClaimOutcome::Acquired(ProcessClaim { lock }))
        }
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            let owner = fs::read(&owner_path)
                .ok()
                .and_then(|record| serde_json::from_slice(&record).ok());
            Ok(ClaimOutcome::Occupied(owner))
        }
        Err(error) => Err(error),
    }
}

impl Drop for ProcessClaim {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
    }
}

#[cfg(test)]
mod tests;
