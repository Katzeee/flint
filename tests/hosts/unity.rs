use crate::support::*;
use anyhow::Result;
use serde_json::Value;
use std::time::Duration;

pub fn verify_scene_execution(app: &App, id: &str) -> Result<()> {
    let workflow = app.workflow("unity-mono-validation")?;
    let scene = unity_execution(app, id, &workflow,
        "var item = new GameObject(\"Flint Unity validation\");\nDebug.Log(\"UNITY_SCENE_OK \" + System.Diagnostics.Process.GetCurrentProcess().Id);\nUnityEngine.Object.DestroyImmediate(item);")?;
    assert_eq!(scene["status"], "succeeded", "{scene:?}");
    assert!(scene["stdout"].as_str().unwrap().contains("UNITY_SCENE_OK"));
    Ok(())
}

pub fn verify_execution_failures(app: &App, id: &str) -> Result<()> {
    let workflow = app.workflow("unity-execution-failures")?;
    let failure = unity_execution(
        app,
        id,
        &workflow,
        "throw new InvalidOperationException(\"UNITY_EXPECTED_FAILURE\");",
    )?;
    assert_eq!(failure["status"], "failed", "{failure:?}");
    assert_eq!(failure["error"], "execution_failed", "{failure:?}");
    assert!(failure["traceback"]
        .as_str()
        .unwrap()
        .contains("UNITY_EXPECTED_FAILURE"));
    let syntax = unity_execution(app, id, &workflow, "this is not valid C#;")?;
    assert_eq!(syntax["status"], "failed", "{syntax:?}");
    assert_eq!(syntax["error"], "preparation_failed", "{syntax:?}");
    assert!(
        syntax["traceback"].as_str().unwrap().contains("error CS"),
        "{syntax:?}"
    );
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
