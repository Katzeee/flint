//! Local endpoints, storage paths, and the user's exclusive backend runtime.
use anyhow::{Context, Result};
use fs2::FileExt;
use std::{
    fs::{File, OpenOptions},
    io,
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub address: String,
    pub control_port: u16,
    pub bridge_port: u16,
    pub timeout: f64,
    pub state_dir: PathBuf,
    runtime_dir: PathBuf,
}

impl Config {
    pub fn load() -> Result<Self> {
        #[cfg(not(feature = "test-runtime"))]
        let (root, control_port, bridge_port) = (
            dirs::data_local_dir()
                .context("cannot locate the user's local data directory")?
                .join("flint"),
            6322,
            6321,
        );
        #[cfg(feature = "test-runtime")]
        let (root, control_port, bridge_port) = test_runtime()?;

        Ok(Self {
            address: "127.0.0.1".into(),
            control_port,
            bridge_port,
            timeout: 30.0,
            state_dir: root.clone(),
            runtime_dir: root.join("runtime"),
        })
    }

    pub fn runtime_dir(&self) -> PathBuf {
        self.runtime_dir.clone()
    }

    pub fn workflows_dir(&self) -> PathBuf {
        self.state_dir.join("workflows")
    }

    pub fn lock_file(&self, name: &str) -> io::Result<File> {
        std::fs::create_dir_all(self.runtime_dir())?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.runtime_dir().join(format!("{name}.lock")))
    }

    /// Claims the backend runtime, or returns `None` while another backend holds it.
    pub fn running_lease(&self) -> io::Result<Option<File>> {
        let file = self.lock_file("running")?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(Some(file)),
            Err(error) if lock_contended(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub fn lock_contended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock || cfg!(windows) && error.raw_os_error() == Some(33)
}

#[cfg(feature = "test-runtime")]
fn test_runtime() -> Result<(PathBuf, u16, u16)> {
    let root = PathBuf::from(
        std::env::var_os("FLINT_TEST_ROOT").context("test build requires FLINT_TEST_ROOT")?,
    );
    anyhow::ensure!(
        root.is_absolute() && root.is_dir(),
        "FLINT_TEST_ROOT must be an existing absolute directory"
    );
    let port = |name| -> Result<u16> {
        let port = std::env::var(name)
            .with_context(|| format!("test build requires {name}"))?
            .parse::<u16>()?;
        anyhow::ensure!(
            port != 0 && port != 6321 && port != 6322,
            "invalid isolated test port: {name}"
        );
        Ok(port)
    };
    let control_port = port("FLINT_TEST_CONTROL_PORT")?;
    let bridge_port = port("FLINT_TEST_BRIDGE_PORT")?;
    anyhow::ensure!(
        control_port != bridge_port,
        "test endpoints must use different ports"
    );
    Ok((root, control_port, bridge_port))
}
