//! Executes the injector's explicit request outside the Windows loader lock.
//! The host does not select configuration or infer exchange paths.
#![cfg(windows)]
mod os;
mod runtimes;
use flint_contracts::attach::{BootstrapRequest, RuntimePlan};

fn run(request: BootstrapRequest) -> u32 {
    let result = match &request.plan {
        RuntimePlan::Cpython(plan) => runtimes::cpython::attach(plan, &request.error_path),
        RuntimePlan::Mono(plan) => runtimes::mono::attach(plan),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = std::fs::write(&request.error_path, format!("{error:#}"));
            1
        }
    }
}
