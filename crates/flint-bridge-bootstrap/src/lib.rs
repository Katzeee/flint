//! The injected attach bootstrap.
//!
//! flint injects this small library into a running host process to start a
//! Bridge from outside, rather than the host loading the Bridge itself. It does
//! the minimum in `DllMain`: spawn a worker thread and return, so no real work
//! runs under the loader lock. The worker reads the per-process config the
//! injector left on disk and drives the host runtime to start the Bridge; the
//! Bridge core loads the ordinary way and takes the process claim, so an already
//! connected host is never given a second Bridge.
//!
//! The injector learns of success from the backend. A failure, here or in the
//! host runtime, is written to `<pid>.error` beside the config for it to report.

#[cfg(windows)]
mod platform;

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Written by the injector to `<temp>/flint-bridge/attach/<pid>.json`, read once
/// by the injected worker. The shape is shared with `flint-hosts`.
#[derive(Deserialize)]
struct AttachConfig {
    /// `cpython` or `dotnet`; selects how the Bridge is started in the host. The
    /// `dotnet` worker probes which managed runtime is loaded (Mono today).
    runtime: String,
    host: String,
    address: String,
    port: u16,
    name: String,
    /// The Bridge package the host runtime loads: the Python ZIP, or the managed
    /// assembly for `dotnet`.
    payload: String,
    /// The native core the managed assembly loads. Present for `dotnet`, where the
    /// managed side loads the core itself; absent for `cpython`, where the Python
    /// package carries it.
    #[serde(default)]
    core_path: Option<String>,
}

fn attach_directory() -> PathBuf {
    std::env::temp_dir().join("flint-bridge").join("attach")
}

fn config_path(pid: u32) -> PathBuf {
    attach_directory().join(format!("{pid}.json"))
}

fn error_path(pid: u32) -> PathBuf {
    attach_directory().join(format!("{pid}.error"))
}

/// Read the config the injector left for this process and start the Bridge.
///
/// Runs on the worker thread, outside the loader lock.
fn run(pid: u32) {
    let path = config_path(pid);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let _ = std::fs::remove_file(&path);
    let result = match serde_json::from_str::<AttachConfig>(&text) {
        Ok(config) => start(&config, &error_path(pid)),
        Err(error) => Err(format!("invalid attach config: {error}")),
    };
    if let Err(error) = result {
        let _ = std::fs::write(error_path(pid), error);
    }
}

/// `error_path` is where a runtime that finishes asynchronously reports failure.
fn start(config: &AttachConfig, error_path: &Path) -> Result<(), String> {
    match config.runtime.as_str() {
        #[cfg(windows)]
        "cpython" => platform::attach_cpython(config, error_path),
        #[cfg(windows)]
        "dotnet" => platform::attach_dotnet(config),
        #[cfg(not(windows))]
        "cpython" | "dotnet" => {
            let _ = error_path;
            Err("attach is only implemented on Windows".into())
        }
        "mono" => Err("unknown attach runtime: mono (use dotnet)".into()),
        other => Err(format!("unknown attach runtime: {other}")),
    }
}

/// Build the Python source that starts the Bridge on a daemon thread.
///
/// The daemon thread is essential: `flint_bridge.attach` marshals onto the host
/// main thread and waits, so it must not run on the injected thread while that
/// thread holds the GIL, or the main thread could never make progress. Its
/// failure is written to `error_path`. JSON string encoding yields valid Python
/// string literals for every field.
fn python_bootstrap(config: &AttachConfig, error_path: &Path) -> String {
    let zip = serde_json::to_string(&config.payload).unwrap();
    let host = serde_json::to_string(&config.host).unwrap();
    let address = serde_json::to_string(&config.address).unwrap();
    let name = serde_json::to_string(&config.name).unwrap();
    let error_path = serde_json::to_string(&error_path.to_string_lossy()).unwrap();
    format!(
        "import sys, threading\n\
         if {zip} not in sys.path:\n    sys.path.insert(0, {zip})\n\
         def _flint_attach():\n    \
         try:\n        \
         import flint_bridge\n        \
         flint_bridge.attach(host={host}, address={address}, port={port}, name={name})\n    \
         except BaseException as error:\n        \
         with open({error_path}, 'w', encoding='utf-8') as report:\n            \
         report.write(str(error) or type(error).__name__)\n\
         threading.Thread(target=_flint_attach, name='flint-attach', daemon=True).start()\n",
        zip = zip,
        host = host,
        address = address,
        name = name,
        port = config.port,
        error_path = error_path,
    )
}

#[cfg(test)]
mod tests;
