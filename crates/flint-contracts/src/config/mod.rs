//! Local endpoints, the control timeout, and the backend's state directory.
mod state;
#[cfg(feature = "test-runtime")]
use anyhow::Context;
use anyhow::Result;
pub use state::{Runtime, StateDir};
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    path::PathBuf,
    time::Duration,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub endpoints: Endpoints,
    pub timeout: Duration,
    pub state: StateDir,
}

#[derive(Debug, Clone, Copy)]
pub struct Endpoints {
    pub control: SocketAddr,
    pub bridge: SocketAddr,
}

impl Endpoints {
    pub fn local(control_port: u16, bridge_port: u16) -> Self {
        Self {
            control: (Ipv4Addr::LOCALHOST, control_port).into(),
            bridge: (Ipv4Addr::LOCALHOST, bridge_port).into(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        #[cfg(not(feature = "test-runtime"))]
        let (root, control_port, bridge_port) = (user_directory()?, 6322, 6321);
        #[cfg(feature = "test-runtime")]
        let (root, control_port, bridge_port) = test_runtime()?;

        Ok(Self {
            endpoints: Endpoints::local(control_port, bridge_port),
            timeout: Duration::from_secs(30),
            state: StateDir::new(root),
        })
    }
}

#[cfg(feature = "test-runtime")]
fn test_runtime() -> Result<(PathBuf, u16, u16)> {
    let root = PathBuf::from(std::env::var_os("FLINT_TEST_ROOT").context("test build requires FLINT_TEST_ROOT")?);
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
    anyhow::ensure!(control_port != bridge_port, "test endpoints must use different ports");
    Ok((root, control_port, bridge_port))
}

fn user_directory() -> io::Result<PathBuf> {
    dirs::data_local_dir()
        .map(|root| root.join("flint"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "cannot locate the user's local data directory"))
}

/// All Bridge copies in a host share this location, independent of the backend
/// endpoint and isolated test runtime. Ownership is scoped to the host PID.
pub fn claim_directory() -> io::Result<PathBuf> {
    Ok(user_directory()?.join("runtime").join("claims"))
}
