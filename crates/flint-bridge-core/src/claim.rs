//! One Bridge per host process.
//!
//! A host process owns at most one Bridge, however many times a runtime loads
//! this library, and however many library versions coexist in the process. The
//! claim is an operating-system advisory lock keyed by a process-wide scope, so
//! it is visible across independent library copies and across language runtimes,
//! and it releases automatically when the process exits. `fs2` provides the lock
//! on every platform this library targets, so the invariant needs no per-OS code.

use fs2::FileExt;
use std::{fs, io, path::PathBuf};

/// Bumped when the recorded owner descriptor changes shape. It is the only
/// contract shared between library versions that may meet in one process.
const LAYOUT_VERSION: u32 = 1;

/// The result of attempting to become a process's single Bridge owner.
pub enum ClaimOutcome {
    /// This caller now owns the process's Bridge until the guard is dropped.
    Acquired(ProcessClaim),
    /// Another live owner holds the process's Bridge; its descriptor when readable.
    Occupied(Option<String>),
}

/// Holds the process's Bridge claim. Dropping it releases the lock.
pub struct ProcessClaim {
    lock: fs::File,
}

fn directory() -> PathBuf {
    // A single process shares one temporary directory across its runtimes, which
    // is the same base the host binding uses to extract this library.
    std::env::temp_dir().join("flint-bridge").join("claims")
}

/// Claim the Bridge for `scope`, recording `owner` for a later caller to read.
///
/// `scope` identifies the process; production callers pass the process id, and a
/// single test process passes a distinct scope per simulated host.
pub fn acquire(scope: &str, owner: &str) -> io::Result<ClaimOutcome> {
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
            let record = format!("{{\"layout_version\":{LAYOUT_VERSION},\"owner\":{owner}}}");
            // The descriptor is a diagnostic aid; failing to record it must not
            // forfeit an otherwise valid claim.
            let _ = fs::write(&owner_path, record);
            Ok(ClaimOutcome::Acquired(ProcessClaim { lock }))
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Ok(ClaimOutcome::Occupied(fs::read_to_string(&owner_path).ok()))
        }
        Err(error) => Err(error),
    }
}

impl Drop for ProcessClaim {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock);
    }
}
