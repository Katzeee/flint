mod support;
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
use support::*;

struct Evidence {
    path: PathBuf,
    data: Value,
}
impl Evidence {
    fn checkpoint(&mut self, name: &str) -> Result<()> {
        self.data["checks"]
            .as_array_mut()
            .unwrap()
            .push(name.into());
        fs::write(&self.path, serde_json::to_vec_pretty(&self.data)?)?;
        Ok(())
    }
}
impl Drop for Evidence {
    fn drop(&mut self) {
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.data) {
            let _ = fs::write(&self.path, bytes);
        }
    }
}

fn verify_host(
    kind: &str,
    variable: &str,
    expected_executable: &str,
    scene_code: &str,
) -> Result<()> {
    let executable = PathBuf::from(
        env::var_os(variable).unwrap_or_else(|| panic!("Set {variable} to the host executable")),
    );
    anyhow::ensure!(
        executable.is_file(),
        "Host executable does not exist: {}",
        executable.display()
    );
    anyhow::ensure!(
        executable
            .file_name()
            .unwrap()
            .to_string_lossy()
            .eq_ignore_ascii_case(expected_executable),
        "Unexpected host executable"
    );
    let app = App::evidence(kind);
    let mut evidence = Evidence {
        path: app.directory.join("result.json"),
        data: json!({"host":kind,"checks":[],"passed":false}),
    };
    assert_eq!(app.call("stop", &[], 0)?["already_stopped"], true);
    assert_eq!(app.call("instances", &[], 0)?["instances"], json!([]));
    let before = app.call("status", &[], 0)?;
    evidence.checkpoint("business_command_starts_backend")?;
    let bundle = app.export()?;
    let ready = app.directory.join("ready.json");
    let bootstrap = app.directory.join("bootstrap.py");
    let config = json!({"host":kind,"bundle":bundle,"port":app.registry_port,"ready":ready});
    let script = format!(
        "import json\nCONFIG = json.loads({})\n{}",
        serde_json::to_string(&config.to_string())?,
        include_str!("../fixtures/dcc_bootstrap.py")
    );
    fs::write(&bootstrap, script)?;
    let mut command = Command::new(executable);
    command
        .current_dir(&app.directory)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME")
        .env_remove("QT_QPA_PLATFORM")
        .stdin(Stdio::null())
        .stdout(fs::File::create(app.directory.join("host.stdout"))?)
        .stderr(fs::File::create(app.directory.join("host.stderr"))?);
    if kind == "maya" {
        command
            .env("MAYA_APP_DIR", app.directory.join("maya-profile"))
            .env("MAYA_DISABLE_CIP", "1")
            .env("MAYA_DISABLE_CER", "1");
        let python_code = format!(
            "import runpy; runpy.run_path({})",
            serde_json::to_string(&bootstrap.to_string_lossy().replace('\\', "/"))?
        );
        command.args([
            "-command",
            &format!("python({});", serde_json::to_string(&python_code)?),
        ]);
    } else {
        command.args(["-q", "-U", "PythonHost"]).arg(bootstrap);
    }
    hidden(&mut command);
    let mut host = OwnedProcess(command.spawn()?);
    evidence.data["host_pid"] = host.0.id().into();
    println!("HOST_STARTED {kind} {}", host.0.id());
    let mut report = None;
    wait_until(Duration::from_secs(180), || {
        anyhow::ensure!(
            host.0.try_wait()?.is_none(),
            "Host exited; inspect {}",
            app.directory.display()
        );
        report = fs::read(&ready)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        Ok(report.is_some())
    })?;
    let report = report.unwrap();
    evidence.data["bootstrap"] = report.clone();
    anyhow::ensure!(report.get("error").is_none(), "Bootstrap failed: {report}");
    assert_eq!(report["pid"], host.0.id());
    assert_eq!(report["main_thread"], true);
    assert_eq!(report["scene"], "");
    let instance = app.await_instance(kind, None)?;
    let id = instance["instance_id"].as_str().unwrap();
    assert_eq!(instance["pid"], host.0.id());
    evidence.checkpoint("native_startup_and_registration")?;
    let workflow = app.workflow(&format!("{kind}-rust-validation"))?;
    let scene = app.execute(id, &workflow, scene_code, 0)?;
    assert_eq!(scene["status"], "succeeded");
    let scene_detail = app.details(&workflow, &scene, 0)?;
    assert!(scene_detail["stdout"]
        .as_str()
        .unwrap()
        .contains(&format!("SCENE_OK {}", host.0.id())));
    assert!(scene_detail["stderr"]
        .as_str()
        .unwrap()
        .contains("STDERR_OK"));
    evidence.data["scene"] = scene_detail.clone();
    evidence.checkpoint("main_thread_scene_crud_and_output")?;
    let failure = app.execute(
        id,
        &workflow,
        "raise ValueError('HOST_EXPECTED_FAILURE')",
        1,
    )?;
    assert!(app.details(&workflow, &failure, 1)?["traceback"]
        .as_str()
        .unwrap()
        .contains("HOST_EXPECTED_FAILURE"));
    evidence.checkpoint("exception_recording")?;
    let long = app.execute(
        id,
        &workflow,
        "import time\nprint('LONG_BEGIN', flush=True)\ntime.sleep(8)\nprint('LONG_DONE')",
        0,
    )?;
    assert_eq!(long["status"], "running");
    assert_eq!(app.call("stop", &[], 1)?["error_code"], "backend_busy");
    wait_until(Duration::from_secs(15), || {
        Ok(app.details(&workflow, &long, 0)?["status"] == "succeeded")
    })?;
    let long_detail = app.details(&workflow, &long, 0)?;
    assert_eq!(long_detail["stdout"], "LONG_BEGIN\nLONG_DONE\n");
    evidence.checkpoint("long_execution_and_busy_shutdown")?;
    let after = app.call("restart", &[], 0)?;
    assert_ne!(before["backend_id"], after["backend_id"]);
    assert!(host.0.try_wait()?.is_none());
    let connected = app.await_instance(kind, Some(id))?;
    assert_eq!(app.details(&workflow, &scene, 0)?, scene_detail);
    let reconnected=app.execute(connected["instance_id"].as_str().unwrap(),&workflow,
        "from PySide2 import QtCore, QtWidgets\nassert QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()\nimport os\nprint('RECONNECTED', os.getpid())",0)?;
    assert!(app.details(&workflow, &reconnected, 0)?["stdout"]
        .as_str()
        .unwrap()
        .contains(&format!("RECONNECTED {}", host.0.id())));
    evidence.checkpoint("backend_restart_reconnect_and_persistence")?;
    assert_eq!(app.call("stop", &[], 0)?["stopped"], true);
    assert!(host.0.try_wait()?.is_none());
    evidence.checkpoint("backend_stop_preserves_host")?;
    evidence.data["passed"] = true.into();
    println!("VALIDATION_PASSED {kind} {}", evidence.path.display());
    Ok(())
}

#[test]
#[ignore = "Set FLINT_MAYA_EXE to a licensed Maya installation"]
fn maya_active_connection() -> Result<()> {
    verify_host(
        "maya",
        "FLINT_MAYA_EXE",
        "maya.exe",
        include_str!("../fixtures/maya_scene.py"),
    )
}
#[test]
#[ignore = "Set FLINT_MAX_EXE to a licensed 3ds Max installation"]
fn max_active_connection() -> Result<()> {
    verify_host(
        "max",
        "FLINT_MAX_EXE",
        "3dsmax.exe",
        include_str!("../fixtures/max_scene.py"),
    )
}
