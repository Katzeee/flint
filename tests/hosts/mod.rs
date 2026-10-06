mod injection;
mod package;
mod unity;

use anyhow::{Context, Result};
use std::{env, path::PathBuf};

fn host_executable(variable: &str) -> Result<PathBuf> {
    let executable = env::var_os(variable)
        .map(PathBuf::from)
        .with_context(|| format!("{variable} must point to the host executable"))?;
    anyhow::ensure!(
        executable.is_file(),
        "Host executable does not exist: {}",
        executable.display()
    );
    Ok(executable)
}
