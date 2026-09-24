use super::dcc::host_executable;
use crate::support::*;
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
#[test]
#[ignore = "real host: set FLINT_UNITY_EXE and run `cargo xtask test hosts`"]
fn unity_active_connection() -> Result<()> {
    let executable = host_executable("FLINT_UNITY_EXE")?;
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
    let project = app.directory.join("unity-project");
    fs::create_dir_all(project.join("Assets"))?;
    fs::create_dir_all(project.join("Assets/Editor"))?;
    fs::create_dir_all(project.join("ProjectSettings"))?;
    fs::create_dir_all(project.join("Packages"))?;
    fs::write(
        project.join("ProjectSettings/ProjectVersion.txt"),
        "m_EditorVersion: 2022.3.62f1\n",
    )?;
    fs::write(
        project.join("Assets/Editor/FlintTestBootstrap.cs"),
        format!(
            r#"using UnityEditor;

[InitializeOnLoad]
public static class FlintTestBootstrap
{{
    static FlintTestBootstrap() {{ EditorApplication.update += Apply; }}

    static void Apply()
    {{
        if (Flint.Unity.EditorBridge.StatusJson == null) return;
        Flint.Unity.EditorBridge.ApplySettings("127.0.0.1", {}, "Unity Editor", true);
        EditorApplication.update -= Apply;
    }}
}}
"#,
            app.registry_port
        ),
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
