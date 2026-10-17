use crate::hosts::host_executable;
use crate::support::*;
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};
#[test]
#[ignore = "real host: set FLINT_UNITY_EXE and run `cargo xtask test hosts`"]
fn exported_package_registers_and_executes() -> Result<()> {
    let executable = host_executable("FLINT_UNITY_EXE")?;
    let app = App::evidence("unity");
    app.call("start", &[], 0)?;
    let bundle = app.export("unity")?;
    let unity_path = |path: &std::path::Path| PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\"));
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
    fs::copy(
        fixture("unity_preferences.cs"),
        package.join("Editor/TestPreferences.cs"),
    )?;
    fs::copy(
        fixture("unity_package_lifecycle.cs"),
        package.join("Editor/TestLifecycle.cs"),
    )?;
    let lifecycle = app.directory.join("lifecycle.txt");
    let reload = app.directory.join("reload");
    let package_path = package.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    fs::write(
        project.join("Packages/manifest.json"),
        serde_json::to_vec(&json!({"dependencies":{"com.flint.bridge":format!("file:{package_path}")}}))?,
    )?;
    let log = app.directory.join("unity.log");
    let mut command = Command::new(executable);
    command
        .args(["-batchmode", "-nographics", "-projectPath"])
        .arg(unity_path(&project))
        .arg("-logFile")
        .arg(unity_path(&log))
        .env("FLINT_TEST_BRIDGE_PORT", app.bridge_port.to_string())
        .env("FLINT_TEST_LIFECYCLE_RESULT", unity_path(&lifecycle))
        .env("FLINT_TEST_RELOAD_TRIGGER", unity_path(&reload))
        .current_dir(unity_path(&project))
        .stdin(Stdio::null())
        .stdout(fs::File::create(app.directory.join("unity.stdout"))?)
        .stderr(fs::File::create(app.directory.join("unity.stderr"))?);
    hidden(&mut command);
    let mut host = OwnedProcess(command.spawn()?);
    println!("UNITY_STARTED {} {}", host.0.id(), app.directory.display());
    let mut instance = None;
    wait_until(Duration::from_secs(180), || {
        anyhow::ensure!(host.0.try_wait()?.is_none(), "Unity exited; inspect {}", log.display());
        if !lifecycle.is_file() {
            return Ok(false);
        }
        let result = fs::read_to_string(&lifecycle)?;
        anyhow::ensure!(result == "ok", "Unity connection lifecycle failed: {result}");
        instance = app.instance("unity")?;
        Ok(instance.is_some())
    })?;
    let instance = instance.unwrap();
    let id = instance["instance_id"].as_str().unwrap();
    let attached = app.call(
        "attach",
        &[
            "--pid",
            &host.0.id().to_string(),
            "--host-kind",
            "unity",
            "--name",
            "Unity Editor",
        ],
        0,
    )?;
    assert_eq!(attached["instance_id"], instance["instance_id"]);
    assert_eq!(instance["pid"], host.0.id());
    assert!(
        instance["candidate"]["executable"].is_string(),
        "discovery did not recognize the connected Editor: {instance}"
    );
    crate::hosts::unity::verify_scene_execution(&app, id)?;
    crate::hosts::unity::verify_execution_failures(&app, id)?;

    // Attaching to the package's Bridge leaves reloads to the package, so the
    // reloaded domain must resolve the adapter from the package alone.
    let loads = app.directory.join("reload.loads");
    let load_count = || fs::read_to_string(&loads).map(|text| text.lines().count()).unwrap_or(0);
    let before_reload = load_count();
    fs::write(&reload, "")?;
    wait_until(Duration::from_secs(120), || {
        anyhow::ensure!(host.0.try_wait()?.is_none(), "Unity exited; inspect {}", log.display());
        Ok(load_count() > before_reload
            && app
                .instance("unity")?
                .is_some_and(|instance| instance["instance_id"] == id))
    })
    .context("package Bridge recovery after script reload")?;
    let workflow = app.workflow("unity-package-reload")?;
    let adapters = crate::hosts::unity::unity_execution(
        &app,
        id,
        &workflow,
        "var found = new System.Collections.Generic.List<string>();\n\
         foreach (var assembly in AppDomain.CurrentDomain.GetAssemblies())\n    \
         if (assembly.GetName().Name == \"Flint.Unity\") found.Add(assembly.Location);\n\
         Debug.Log(\"FLINT_ADAPTERS \" + string.Join(\"|\", found.ToArray()) + \" FLINT_END\");",
    )?;
    assert_eq!(adapters["status"], "succeeded", "{adapters:?}");
    let stdout = adapters["stdout"].as_str().unwrap();
    let locations = stdout
        .split_once("FLINT_ADAPTERS ")
        .and_then(|(_, rest)| rest.split_once(" FLINT_END"))
        .map(|(locations, _)| locations.replace('\\', "/").to_lowercase())
        .with_context(|| format!("adapter report missing: {stdout}"))?;
    let locations: Vec<_> = locations.split('|').collect();
    anyhow::ensure!(
        locations.len() == 1 && locations[0].starts_with(&package_path.to_lowercase()),
        "the reloaded domain resolved adapters {locations:?}, expected one from {package_path}"
    );
    assert!(host.0.try_wait()?.is_none());
    println!("VALIDATION_PASSED unity {}", app.directory.display());
    Ok(())
}
