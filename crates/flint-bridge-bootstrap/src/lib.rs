//! The injected attach bootstrap.
//!
//! flint injects this small library into a running host process to start a
//! Bridge from outside, rather than the host loading the Bridge itself. It does
//! the minimum in `DllMain`: spawn a worker thread and return, so no real work
//! runs under the loader lock. The worker reads the per-process plan the
//! injector left on disk and drives the host runtime to start the Bridge; the
//! Bridge core loads the ordinary way and takes the process claim, so an already
//! connected host is never given a second Bridge.
//!
//! The injector learns of success from the backend. A failure here is written
//! to `<pid>.error` beside the plan; work the plan schedules reports its own.

#![cfg(windows)]

mod os;
mod runtimes;

use anyhow::Result;
use flint_contracts::attach::{error_path, plan_path, RuntimePlan};

/// Read the plan the injector left for this process and start the Bridge.
///
/// Runs on the worker thread, outside the loader lock.
fn run(pid: u32) {
    let path = plan_path(pid);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let _ = std::fs::remove_file(&path);
    let result = match serde_json::from_str::<RuntimePlan>(&text) {
        Ok(plan) => start(&plan),
        Err(error) => Err(anyhow::anyhow!("invalid runtime plan: {error}")),
    };
    if let Err(error) = result {
        let _ = std::fs::write(error_path(pid), format!("{error:#}"));
    }
}

fn start(plan: &RuntimePlan) -> Result<()> {
    match plan {
        RuntimePlan::Cpython(plan) => runtimes::cpython::attach(plan),
        RuntimePlan::Mono(plan) => runtimes::mono::attach(plan),
    }
}
