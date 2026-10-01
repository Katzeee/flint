use crate::support::*;
use anyhow::Result;
use serde_json::Value;
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};

/// Attach into a plain CPython process with flint's own injector — no host
/// application and no third-party injector — then execute through it and confirm
/// a second attach is idempotent. This is Windows-only and does real process
/// injection, so it runs in the host suite.
#[cfg(windows)]
#[test]
#[ignore = "real injection: run `cargo xtask test hosts` on Windows"]
fn attaches_a_plain_python_process_and_executes() -> Result<()> {
    let app = App::evidence("attach-python");
    app.call("start", &[], 0)?;

    // Resolve the real interpreter executable: a venv launcher (as uv builds)
    // is a trampoline that spawns the interpreter as a child, so injecting into
    // the launcher finds no runtime. Real hosts embed the interpreter in-process.
    let interpreter = {
        let output = Command::new(python())
            .args([
                "-I",
                "-c",
                "import sys; print(getattr(sys, '_base_executable', sys.executable))",
            ])
            .output()?;
        anyhow::ensure!(
            output.status.success(),
            "cannot resolve the base interpreter"
        );
        std::path::PathBuf::from(String::from_utf8(output.stdout)?.trim())
    };

    // A sleeper that signals readiness once its interpreter is initialized. Its
    // output is captured so a failed injection can be diagnosed.
    let ready = app.directory.join("target-ready");
    let target_err = app.directory.join("target.stderr");
    let mut command = Command::new(&interpreter);
    command
        .args([
            "-I",
            "-c",
            "import sys,time; open(sys.argv[1],'w').close(); time.sleep(120)",
        ])
        .arg(&ready)
        .current_dir(&app.directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(&target_err)?);
    hidden(&mut command);
    let target = OwnedProcess(command.spawn()?);
    let pid = target.0.id();
    wait_until(Duration::from_secs(15), || Ok(ready.is_file()))?;

    let attached = app
        .call(
            "attach",
            &[
                "--pid",
                &pid.to_string(),
                "--host-kind",
                "python",
                "--name",
                "Injected",
            ],
            0,
        )
        .map_err(|error| {
            let status = fs::read_to_string(
                std::env::temp_dir()
                    .join("flint-bridge")
                    .join("attach")
                    .join(format!("{pid}.status")),
            )
            .unwrap_or_else(|_| "<no bootstrap status>".into());
            let stderr = fs::read_to_string(&target_err).unwrap_or_default();
            anyhow::anyhow!("{error}\nbootstrap status: {status}\ntarget stderr: {stderr}")
        })?;
    anyhow::ensure!(attached["attached"] == true, "attach failed: {attached}");
    anyhow::ensure!(
        attached["pid"].as_u64() == Some(pid as u64),
        "wrong pid: {attached}"
    );
    let instance = attached["instance_id"].as_str().unwrap().to_string();

    // The injected Bridge executes host code and reports the target's own pid.
    let workflow = app.workflow("Attach")?;
    let execution = app.execute(
        &instance,
        &workflow,
        "import os; print('attached', os.getpid())",
        0,
    )?;
    let mut detail = Value::Null;
    wait_until(Duration::from_secs(30), || {
        detail = app.details(&workflow, &execution, 0)?;
        Ok(matches!(
            detail["status"].as_str(),
            Some("succeeded" | "failed")
        ))
    })?;
    anyhow::ensure!(
        detail["status"] == "succeeded",
        "execution failed: {detail}"
    );
    anyhow::ensure!(
        detail["stdout"]
            .as_str()
            .unwrap()
            .contains(&format!("attached {pid}")),
        "unexpected output: {detail}"
    );

    // Re-attaching the same process reuses the instance and adds no second one.
    let again = app.call(
        "attach",
        &["--pid", &pid.to_string(), "--host-kind", "python"],
        0,
    )?;
    anyhow::ensure!(
        again["instance_id"] == instance.as_str(),
        "re-attach changed instance: {again}"
    );
    let instances = app.call("instances", &["--type", "python"], 0)?;
    anyhow::ensure!(
        instances["instances"].as_array().unwrap().len() == 1,
        "expected one instance: {instances}"
    );
    Ok(())
}
