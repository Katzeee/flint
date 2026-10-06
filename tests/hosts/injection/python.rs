use crate::support::*;
use anyhow::Result;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

/// The real interpreter executable: a venv launcher (as uv builds) is a
/// trampoline that spawns the interpreter as a child, so injecting into the
/// launcher finds no runtime. Real hosts embed the interpreter in-process.
fn base_interpreter() -> Result<PathBuf> {
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
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

/// Start a target interpreter that runs `setup`, then signals readiness and
/// sleeps. Its stderr is captured so a failed injection can be diagnosed.
fn start_target(app: &App, setup: &str) -> Result<OwnedProcess> {
    let ready = app.directory.join("target-ready");
    let script =
        format!("import sys, time\n{setup}\nopen(sys.argv[1], 'w').close()\ntime.sleep(120)\n");
    let mut command = Command::new(base_interpreter()?);
    command
        .args(["-I", "-c", &script])
        .arg(&ready)
        .current_dir(&app.directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(fs::File::create(app.directory.join("target.stderr"))?);
    hidden(&mut command);
    let mut target = OwnedProcess(command.spawn()?);
    wait_until(Duration::from_secs(15), || {
        anyhow::ensure!(
            target.0.try_wait()?.is_none(),
            "target exited: {}",
            fs::read_to_string(app.directory.join("target.stderr"))?
        );
        Ok(ready.is_file())
    })?;
    Ok(target)
}

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
    let target = start_target(&app, "")?;
    let pid = target.0.id();

    let attached = app
        .call(
            "attach",
            &[
                "--pid",
                &pid.to_string(),
                "--host-kind",
                "standalone_python",
                "--name",
                "Injected",
            ],
            0,
        )
        .map_err(|error| {
            let stderr =
                fs::read_to_string(app.directory.join("target.stderr")).unwrap_or_default();
            anyhow::anyhow!("{error}\ntarget stderr: {stderr}")
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

    // Re-attaching with the same configuration reuses the instance and adds no
    // second one.
    let again = app.call(
        "attach",
        &[
            "--pid",
            &pid.to_string(),
            "--host-kind",
            "standalone_python",
            "--name",
            "Injected",
        ],
        0,
    )?;
    anyhow::ensure!(
        again["instance_id"] == instance.as_str(),
        "re-attach changed instance: {again}"
    );
    let instances = app.call("instances", &["--type", "standalone_python"], 0)?;
    anyhow::ensure!(
        instances["instances"].as_array().unwrap().len() == 1,
        "expected one instance: {instances}"
    );
    Ok(())
}

/// A process whose claim is held outside the attach path cannot receive a
/// Bridge, and attach reports the injected side's reason instead of timing out.
#[cfg(windows)]
#[test]
#[ignore = "real injection: run `cargo xtask test hosts` on Windows"]
fn attach_reports_why_the_injected_bridge_could_not_start() -> Result<()> {
    let app = App::evidence("attach-python-claimed");
    app.call("start", &[], 0)?;
    let bundle = serde_json::to_string(&app.export()?.to_string_lossy())?;
    // A core created directly, not through the interpreter's Bridge, holds the
    // claim where the injected host Bridge cannot reuse it.
    let target = start_target(
        &app,
        &format!(
            "sys.path.insert(0, {bundle})\n\
             from flint_bridge.connection.native_core import NativeCore\n\
             core = NativeCore({{'host': 'standalone_python', 'address': '127.0.0.1', 'port': 1,\n\
                                 'name': 'holder', 'runtime_version': 'holder', 'enabled': False}})"
        ),
    )?;
    let failed = app.call(
        "attach",
        &[
            "--pid",
            &target.0.id().to_string(),
            "--host-kind",
            "standalone_python",
            "--timeout",
            "60",
        ],
        1,
    )?;
    anyhow::ensure!(failed["attached"] == false, "attach succeeded: {failed}");
    let message = failed["message"].as_str().unwrap_or_default();
    anyhow::ensure!(
        message.contains("Another Bridge already owns this process")
            && message.contains("runtime_version=holder"),
        "unexpected attach failure: {failed}"
    );
    Ok(())
}
