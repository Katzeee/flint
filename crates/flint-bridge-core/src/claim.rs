//! The OS lock enforces one Bridge across independently loaded library copies
//! and language runtimes in the same process. A Rust static cannot do that.

use flint_contracts::{config::claim_directory, lock::FileLock};
use serde::{Deserialize, Serialize};
use std::{fmt, fs, io};

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
    Acquired(FileLock),
    Occupied(Option<ClaimOwner>),
}

pub(crate) fn acquire(owner: &ClaimOwner) -> io::Result<ClaimOutcome> {
    acquire_scope(&std::process::id().to_string(), owner)
}

#[cfg(test)]
pub(crate) fn acquire_for_test(scope: &str, owner: &ClaimOwner) -> io::Result<ClaimOutcome> {
    acquire_scope(scope, owner)
}

fn acquire_scope(scope: &str, owner: &ClaimOwner) -> io::Result<ClaimOutcome> {
    let directory = claim_directory()?;
    let owner_path = directory.join(format!("{scope}.owner"));
    match FileLock::try_acquire(&directory.join(format!("{scope}.lock")))? {
        Some(lock) => {
            // The descriptor is a diagnostic aid; failing to record it must not
            // forfeit an otherwise valid claim.
            if let Ok(record) = serde_json::to_vec(owner) {
                let _ = fs::write(&owner_path, record);
            }
            Ok(ClaimOutcome::Acquired(lock))
        }
        None => {
            let owner = fs::read(&owner_path)
                .ok()
                .and_then(|record| serde_json::from_slice(&record).ok());
            Ok(ClaimOutcome::Occupied(owner))
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::TestScope;
