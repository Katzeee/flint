//! Executes the injector's explicit request outside the Windows loader lock.
//! The host does not select configuration or infer exchange paths.
#![cfg(windows)]
mod os;
mod runtimes;
use flint_contracts::attach::{BootstrapRequest, RuntimePlan, write_report};

fn run(request: BootstrapRequest) -> u32 {
    let result = match &request.plan {
        // The Python entry returns once started and reports the outcome itself.
        RuntimePlan::Cpython(plan) => runtimes::cpython::attach(plan, &request.report_path),
        RuntimePlan::Mono(plan) => runtimes::mono::attach(plan).map(|()| {
            let _ = write_report(&request.report_path, Ok(()));
        }),
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = write_report(&request.report_path, Err(&format!("{error:#}")));
            1
        }
    }
}
