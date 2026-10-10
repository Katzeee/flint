//! Exclusive file locks that separately built processes coordinate through.
use fs2::FileExt;
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};

/// Held until dropped; the operating system also releases it when the process exits.
#[must_use]
#[derive(Debug)]
pub struct FileLock(File);

impl FileLock {
    /// Returns `None` while another holder owns the lock.
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        fs::create_dir_all(path.parent().expect("lock file has a parent directory"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self(file))),
            Err(error) if contended(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

fn contended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock || error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
}
