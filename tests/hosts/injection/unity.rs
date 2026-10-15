use crate::hosts::host_executable;
use crate::support::*;
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

/// A single attach restores the Bridge across script reloads, preserves updated
/// settings, and respects an explicit disconnect. Refused settings leave it usable.
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
            r#"using System;
using System.Linq;
using System.IO;
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
        var command = File.ReadAllText(Trigger);
        File.Delete(Trigger);
        if (command == "quit") {{ EditorApplication.Exit(0); return; }}
        var bridge = AppDomain.CurrentDomain.GetAssemblies()
            .Select(assembly => assembly.GetType("Flint.Unity.EditorBridge")).FirstOrDefault(type => type != null);
        if (command == "rename")
            bridge.GetMethod("ApplySettings").Invoke(null, new object[] {{ "127.0.0.1", {port}, "Reloaded Unity" }});
        if (command == "disconnect" && !(bool)bridge.GetMethod("Disconnect").Invoke(null, null))
            throw new Exception("Idle Bridge refused disconnect");
        EditorUtility.RequestScriptReload();
    }}
}}
"#,
            unity_path(&loads).display(),
            unity_path(&trigger).display(),
            port = app.bridge_port,
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
        .stderr(fs::File::create(app.directory.join("unity.stderr"))?);
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

    for command in ["rename", ""] {
        let before_reload = load_count();
        fs::write(&trigger, command)?;
        wait_until(Duration::from_secs(120), || {
            anyhow::ensure!(host.0.try_wait()?.is_none(), "Unity exited; inspect {}", log.display());
            if load_count() <= before_reload {
                return Ok(false);
            }
            Ok(app
                .instance("unity")?
                .is_some_and(|instance| instance["instance_id"] == first && instance["name"] == "Reloaded Unity"))
        })
        .context("automatic Unity recovery after script reload")?;
        crate::hosts::unity::verify_scene_execution(&app, &first)?;
    }

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
    let instances = app.connected("unity")?;
    anyhow::ensure!(
        instances.len() == 1 && instances[0]["instance_id"] == first.as_str(),
        "the refused attach changed the Bridge: {instances:?}"
    );
    crate::hosts::unity::verify_scene_execution(&app, &first)?;
    let before_reload = load_count();
    fs::write(&trigger, "disconnect")?;
    wait_until(Duration::from_secs(120), || Ok(load_count() > before_reload))?;
    // Observe several Editor updates after the new domain starts. An unwanted
    // restore can register asynchronously, so an immediate empty list is insufficient.
    let stopped = std::time::Instant::now();
    wait_until(Duration::from_secs(10), || {
        anyhow::ensure!(
            app.instance("unity")?.is_none(),
            "explicitly disconnected Bridge restarted after reload"
        );
        Ok(stopped.elapsed() >= Duration::from_secs(3))
    })?;
    anyhow::ensure!(attach()? == first, "reattach changed the process identity");
    crate::hosts::unity::verify_scene_execution(&app, &first)?;
    fs::write(&trigger, "quit")?;
    wait_until(Duration::from_secs(30), || match host.0.try_wait()? {
        Some(status) => {
            anyhow::ensure!(
                status.success(),
                "Unity shutdown failed: {status}; inspect {}",
                log.display()
            );
            Ok(true)
        }
        None => Ok(false),
    })
    .context("Unity shutdown with the reload observer installed")?;
    Ok(())
}
