use crate::hosts::host_executable;
use crate::support::*;
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

/// Attach into a fresh Unity Editor, reload its scripts, and attach again. The
/// reload unloads the domain holding the attached Bridge, so it must release its
/// registration and process claim, and a later injection must start a new Bridge.
/// An attach whose settings that Bridge refuses reports why.
#[cfg(windows)]
#[test]
#[ignore = "real host: set FLINT_UNITY_EXE and run `cargo xtask test hosts`"]
fn attach_survives_a_script_reload_and_reports_refused_settings() -> Result<()> {
    let executable = host_executable("FLINT_UNITY_EXE")?;
    let app = App::evidence("attach-unity");
    app.call("start", &[], 0)?;

    let unity_path = |path: &Path| PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\"));
    let project = app.directory.join("unity-project");
    for folder in ["Assets/Editor", "ProjectSettings", "Packages"] {
        fs::create_dir_all(project.join(folder))?;
    }
    fs::write(project.join("Packages/manifest.json"), r#"{"dependencies":{}}"#)?;
    // Records ready scripting domains in the main Editor, excluding import workers.
    let loads = app.directory.join("loads.txt");
    let trigger = app.directory.join("reload");
    fs::write(
        project.join("Assets/Editor/FlintReload.cs"),
        format!(
            r#"using System.IO;
using UnityEditor;

[InitializeOnLoad]
public static class FlintReload
{{
    const string Loads = @"{}";
    const string Trigger = @"{}";
    static bool ready;

    static FlintReload()
    {{
        if (AssetDatabase.IsAssetImportWorkerProcess()) return;
        EditorApplication.update += Poll;
    }}

    static void Poll()
    {{
        if (!ready)
        {{
            ready = true;
            File.AppendAllText(Loads, "loaded\n");
        }}
        if (!File.Exists(Trigger)) return;
        File.Delete(Trigger);
        EditorUtility.RequestScriptReload();
    }}
}}
"#,
            unity_path(&loads).display(),
            unity_path(&trigger).display()
        ),
    )?;
    let load_count = || -> usize { fs::read_to_string(&loads).map(|text| text.lines().count()).unwrap_or(0) };

    let log = app.directory.join("unity.log");
    let mut command = Command::new(executable);
    command
        .args(["-batchmode", "-nographics", "-projectPath"])
        .arg(unity_path(&project))
        .arg("-logFile")
        .arg(unity_path(&log))
        .current_dir(unity_path(&project))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hidden(&mut command);
    let mut host = OwnedProcess(command.spawn()?);
    let pid = host.0.id();
    wait_until(Duration::from_secs(300), || {
        anyhow::ensure!(host.0.try_wait()?.is_none(), "Unity exited; inspect {}", log.display());
        Ok(load_count() >= 1)
    })?;

    let attach = || -> Result<String> {
        let attached = app.call("attach", &["--pid", &pid.to_string(), "--host-kind", "unity"], 0)?;
        Ok(attached["instance_id"].as_str().unwrap().to_string())
    };
    let first = attach().context("initial Unity attach")?;
    crate::hosts::unity::verify_scene_execution(&app, &first)?;

    let before_reload = load_count();
    fs::write(&trigger, "")?;
    wait_until(Duration::from_secs(120), || Ok(load_count() > before_reload))?;
    // The unloaded domain must take its Bridge with it.
    wait_until(Duration::from_secs(30), || {
        let instances = app.call("instances", &["--type", "unity"], 0)?;
        Ok(instances["instances"].as_array().unwrap().is_empty())
    })?;

    // The released claim and a fresh bootstrap load let a new Bridge start.
    let second = attach().context("Unity attach after script reload")?;
    anyhow::ensure!(second != first, "re-attach reused the unloaded Bridge");

    // Settings the running Bridge refuses come back as the attach's failure and
    // leave that Bridge connected.
    let refused = app.call(
        "attach",
        &["--pid", &pid.to_string(), "--host-kind", "unity", "--name", ""],
        1,
    )?;
    anyhow::ensure!(
        refused["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("Invalid Bridge connection settings")),
        "unexpected attach result: {refused}"
    );
    let instances = app.call("instances", &["--type", "unity"], 0)?;
    anyhow::ensure!(
        instances["instances"][0]["instance_id"] == second.as_str(),
        "the refused attach changed the Bridge: {instances}"
    );
    crate::hosts::unity::verify_scene_execution(&app, &second)?;
    assert!(host.0.try_wait()?.is_none());
    Ok(())
}
