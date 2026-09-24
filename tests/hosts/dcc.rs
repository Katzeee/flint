use crate::support::*;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

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

pub(super) fn host_executable(variable: &str) -> Result<PathBuf> {
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

pub(super) fn verify_host(
    kind: &str,
    variable: &str,
    expected_executable: &str,
    scene_code: &str,
) -> Result<()> {
    let executable = host_executable(variable)?;
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
    app.call("start", &[], 0)?;
    let bundle = if kind == "blender" {
        app.export_blender()?
    } else {
        app.export()?
    };
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
    } else if kind == "max" {
        command.args(["-q", "-U", "PythonHost"]).arg(bootstrap);
    } else {
        let config = app.directory.join("blender-config");
        let scripts = app.directory.join("blender-scripts");
        fs::create_dir_all(&config)?;
        fs::create_dir_all(&scripts)?;
        command
            .env("BLENDER_USER_CONFIG", config)
            .env("BLENDER_USER_SCRIPTS", scripts)
            .env("FLINT_BLENDER_REGISTRY_PORT", app.registry_port.to_string())
            .args(["--factory-startup", "--disable-autoexec", "--python"])
            .arg(bootstrap);
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
    let candidates: Value = serde_json::from_str(
        &checked(
            Command::new(&app.binary).args(["hosts", "--json"]),
            Duration::from_secs(10),
        )?
        .stdout,
    )?;
    assert!(candidates["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|candidate| {
            candidate["pid"] == host.0.id()
                && candidate["host"] == kind
                && candidate["attach_supported"] == false
        }));
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
    evidence.data["scene"] = scene_detail;
    evidence.checkpoint("main_thread_scene_crud_and_output")?;
    assert!(host.0.try_wait()?.is_none());
    evidence.data["passed"] = true.into();
    println!("VALIDATION_PASSED {kind} {}", evidence.path.display());
    Ok(())
}
