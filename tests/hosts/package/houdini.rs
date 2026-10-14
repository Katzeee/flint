use crate::hosts::host_executable;
use crate::support::*;
use anyhow::Result;
use serde_json::json;
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};

#[test]
#[ignore = "real host: set FLINT_HOUDINI_EXE and run `cargo xtask test hosts`"]
fn package_and_attach_execute_on_houdinis_main_thread() -> Result<()> {
    let executable = host_executable("FLINT_HOUDINI_EXE")?;
    for install in [true, false] {
        let app = App::evidence(if install { "houdini-package" } else { "houdini-attach" });
        app.call("start", &[], 0)?;
        let profile = app.directory.join("profile");
        fs::create_dir_all(&profile)?;
        if install {
            let bundle = app.export("houdini")?;
            checked(
                Command::new(python())
                    .args([
                        "-c",
                        "import sys,zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])",
                    ])
                    .arg(bundle)
                    .arg(profile.join("packages")),
                Duration::from_secs(15),
            )?;
            let settings = json!({"address": "127.0.0.1", "port": app.bridge_port, "name": "Houdini test"});
            let startup = format!(
                "from pathlib import Path\nimport hou\nPath(hou.homeHoudiniDirectory()).joinpath('flint.json').write_text({}, encoding='utf-8')\n",
                serde_json::to_string(&settings.to_string())?
            );
            for minor in 9..=14 {
                let path = profile.join(format!("packages/flint/python3.{minor}libs/pythonrc.py"));
                fs::write(&path, format!("{startup}{}", fs::read_to_string(&path)?))?;
            }
        }
        let mut command = Command::new(&executable);
        command
            .current_dir(&app.directory)
            .env("HOUDINI_USER_PREF_DIR", profile.join("__HVER__"))
            .env("HOUDINI_PACKAGE_DIR", profile.join("packages"))
            .env_remove("PYTHONPATH")
            .env_remove("PYTHONHOME")
            .env_remove("QT_QPA_PLATFORM")
            .stdin(Stdio::null())
            .stdout(fs::File::create(app.directory.join("host.stdout"))?)
            .stderr(fs::File::create(app.directory.join("host.stderr"))?);
        hidden(&mut command);
        let mut host = OwnedProcess(command.spawn()?);
        if !install {
            wait_until(Duration::from_secs(90), || {
                anyhow::ensure!(host.0.try_wait()?.is_none(), "Houdini exited before attach");
                let info = app.call("hosts", &["info", "--pid", &host.0.id().to_string()], 0)?;
                Ok(!info["window"].is_null())
            })?;
            app.call("attach", &["--pid", &host.0.id().to_string()], 0)?;
        }
        let mut instance = None;
        wait_until(Duration::from_secs(120), || {
            anyhow::ensure!(host.0.try_wait()?.is_none(), "Houdini exited before registration");
            instance = app.instance("houdini")?;
            Ok(instance.is_some())
        })?;
        let instance = instance.unwrap();
        let id = instance["instance_id"].as_str().unwrap();
        let workflow = app.workflow("houdini")?;
        let execution = app.execute(id, &workflow, include_str!("../../fixtures/houdini_scene.py"), 0)?;
        let detail = app.details(&workflow, &execution, 0)?;
        assert_eq!(detail["stdout"], "HOUDINI_SCENE_OK\n");
        if install {
            let failure = app.execute(id, &workflow, "raise ValueError('HOUDINI_EXPECTED')", 1)?;
            assert_eq!(failure["error"]["code"], "execution_failed");
            let failure = app.execute(id, &workflow, "if :", 1)?;
            assert_eq!(failure["error"]["code"], "preparation_failed");
            assert_eq!(app.execute(id, &workflow, "print('alive')", 0)?["status"], "succeeded");
        }
    }
    Ok(())
}
