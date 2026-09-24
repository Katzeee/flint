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

#[test]
#[ignore = "Set FLINT_UNITY_EXE to a Unity 2022.3 Mono Editor installation"]
fn unity_active_connection() -> Result<()> {
    let executable = PathBuf::from(env::var_os("FLINT_UNITY_EXE").expect("Set FLINT_UNITY_EXE"));
    anyhow::ensure!(executable.is_file(), "Unity executable does not exist");
    let app = App::evidence("unity");
    app.call("start", &[], 0)?;
    let bundle = app.export_unity()?;
    let unity_path =
        |path: &std::path::Path| PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\"));
    checked(
        Command::new("tar")
            .arg("-xzf")
            .arg(unity_path(&bundle))
            .arg("-C")
            .arg(unity_path(&app.directory)),
        Duration::from_secs(15),
    )?;
    let package = app.directory.join("package");
    let bootstrap = package.join("Editor/EditorBootstrap.cs");
    let source = fs::read_to_string(&bootstrap)?;
    let default_port = "private const int RegistryPort = 6321;";
    anyhow::ensure!(
        source.matches(default_port).count() == 1,
        "Missing Unity registry port"
    );
    fs::write(
        &bootstrap,
        source.replace(
            default_port,
            &format!("private const int RegistryPort = {};", app.registry_port),
        ),
    )?;

    let project = app.directory.join("unity-project");
    fs::create_dir_all(project.join("Assets"))?;
    fs::create_dir_all(project.join("ProjectSettings"))?;
    fs::create_dir_all(project.join("Packages"))?;
    fs::write(
        project.join("ProjectSettings/ProjectVersion.txt"),
        "m_EditorVersion: 2022.3.62f1\n",
    )?;
    let package_path = package
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    fs::write(
        project.join("Packages/manifest.json"),
        serde_json::to_vec(
            &json!({"dependencies":{"com.flint.bridge":format!("file:{package_path}")}}),
        )?,
    )?;
    let log = app.directory.join("unity.log");
    let mut command = Command::new(executable);
    command
        .args(["-batchmode", "-nographics", "-projectPath"])
        .arg(unity_path(&project))
        .arg("-logFile")
        .arg(unity_path(&log))
        .current_dir(unity_path(&project))
        .stdin(Stdio::null())
        .stdout(fs::File::create(app.directory.join("unity.stdout"))?)
        .stderr(fs::File::create(app.directory.join("unity.stderr"))?);
    hidden(&mut command);
    let mut host = OwnedProcess(command.spawn()?);
    println!("UNITY_STARTED {} {}", host.0.id(), app.directory.display());
    let mut instance = None;
    wait_until(Duration::from_secs(180), || {
        anyhow::ensure!(
            host.0.try_wait()?.is_none(),
            "Unity exited; inspect {}",
            log.display()
        );
        instance = app.instance("unity")?;
        Ok(instance.is_some())
    })?;
    let instance = instance.unwrap();
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
        .any(|candidate| { candidate["pid"] == host.0.id() && candidate["host"] == "unity" }));
    let workflow = app.workflow("unity-mono-validation")?;
    let scene = unity_execution(&app, id, &workflow,
        "var item = new GameObject(\"Flint Unity validation\");\nDebug.Log(\"UNITY_SCENE_OK \" + System.Diagnostics.Process.GetCurrentProcess().Id);\nUnityEngine.Object.DestroyImmediate(item);")?;
    assert_eq!(scene["status"], "succeeded", "{scene:?}");
    assert!(scene["stdout"].as_str().unwrap().contains("UNITY_SCENE_OK"));
    let failure = unity_execution(
        &app,
        id,
        &workflow,
        "throw new InvalidOperationException(\"UNITY_EXPECTED_FAILURE\");",
    )?;
    assert_eq!(failure["status"], "failed", "{failure:?}");
    assert!(failure["traceback"]
        .as_str()
        .unwrap()
        .contains("UNITY_EXPECTED_FAILURE"));
    let syntax = unity_execution(&app, id, &workflow, "this is not valid C#;")?;
    assert_eq!(syntax["status"], "failed", "{syntax:?}");
    assert_eq!(syntax["error"], "compile_error");
    let before = app.call("status", &[], 0)?;
    let after = app.call("restart", &[], 0)?;
    assert_ne!(before["backend_id"], after["backend_id"]);
    let connected = app.await_instance("unity", Some(id))?;
    let next = unity_execution(
        &app,
        connected["instance_id"].as_str().unwrap(),
        &workflow,
        "Debug.Log(\"UNITY_RECONNECTED\");",
    )?;
    assert_eq!(next["status"], "succeeded", "{next:?}");
    assert!(next["stdout"]
        .as_str()
        .unwrap()
        .contains("UNITY_RECONNECTED"));
    assert!(host.0.try_wait()?.is_none());
    println!("VALIDATION_PASSED unity {}", app.directory.display());
    Ok(())
}

fn unity_execution(app: &App, instance: &str, workflow: &str, code: &str) -> Result<Value> {
    let submitted = run(
        app.command("exec").args([
            "--instance-id",
            instance,
            "--workflow-id",
            workflow,
            "--code",
            code,
        ]),
        Duration::from_secs(60),
        None,
    )?;
    anyhow::ensure!(
        submitted
            .status
            .code()
            .is_some_and(|code| code == 0 || code == 1),
        "Unity submission failed: {} {}",
        submitted.stdout,
        submitted.stderr
    );
    let accepted: Value = serde_json::from_str(&submitted.stdout)?;
    let execution = accepted["execution_id"].as_str().unwrap();
    let mut detail = None;
    wait_until(Duration::from_secs(60), || {
        let response = run(
            app.command("execution").args([
                "--workflow-id",
                workflow,
                "--execution-id",
                execution,
                "--view",
                "full",
            ]),
            Duration::from_secs(15),
            None,
        )?;
        anyhow::ensure!(
            response
                .status
                .code()
                .is_some_and(|code| code == 0 || code == 1),
            "Unity execution lookup failed: {} {}",
            response.stdout,
            response.stderr
        );
        detail = Some(serde_json::from_str::<Value>(&response.stdout)?);
        Ok(detail.as_ref().unwrap()["status"] != "running")
    })?;
    Ok(detail.unwrap())
}
