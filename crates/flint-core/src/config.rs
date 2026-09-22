use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    path::PathBuf,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub registry_host: String,
    pub registry_port: u16,
    pub timeout: f64,
    pub state_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 6322,
            registry_host: "127.0.0.1".into(),
            registry_port: 6321,
            timeout: 30.0,
            state_dir: std::env::var_os("FLINT_STATE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    dirs::data_local_dir()
                        .unwrap_or_else(std::env::temp_dir)
                        .join("flint")
                }),
        }
    }
}

impl Config {
    pub fn runtime_dir(&self) -> PathBuf {
        self.state_dir.join("runtime").join(self.port.to_string())
    }
    pub fn lock_file(&self, name: &str) -> std::io::Result<File> {
        std::fs::create_dir_all(self.runtime_dir())?;
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.runtime_dir().join(format!("{name}.lock")))
    }
    pub fn running_lease(&self) -> anyhow::Result<File> {
        let file = self.lock_file("running")?;
        file.try_lock_exclusive()
            .map_err(|_| anyhow::anyhow!("backend_locked: another backend owns this endpoint"))?;
        Ok(file)
    }
}
