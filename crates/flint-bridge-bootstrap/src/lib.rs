//! Executes the injector's explicit request outside the Windows loader lock.
//! The host does not select configuration or infer exchange paths.
#![cfg(windows)]
mod os;
mod runtimes;
use anyhow::{Context, Result};
use flint_contracts::attach::{BootstrapRequest, RuntimePlan};

fn run(request: BootstrapRequest) -> u32 {
    let result = (|| -> Result<()> {
        let text = std::fs::read_to_string(&request.plan_path)
            .with_context(|| format!("cannot read attach plan {}", request.plan_path.display()))?;
        let _ = std::fs::remove_file(&request.plan_path);
        let plan = serde_json::from_str::<RuntimePlan>(&text).context("invalid runtime plan")?;
        start(&plan)
    })();
    match result {
        Ok(()) => 0,
        Err(error) => {
            let _ = std::fs::write(&request.error_path, format!("{error:#}"));
            1
        }
    }
}

fn start(plan: &RuntimePlan) -> Result<()> {
    match plan {
        RuntimePlan::Cpython(plan) => runtimes::cpython::attach(plan),
        RuntimePlan::Mono(plan) => runtimes::mono::attach(plan),
    }
}

#[cfg(test)]
mod tests;
