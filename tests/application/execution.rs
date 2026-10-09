use crate::support::*;
use anyhow::Result;
use serde_json::json;
use std::{fs, time::Duration};

#[test]
fn execution_errors_are_recorded_without_terminating_host() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("errors")?;
    let execution = app.execute(id, &workflow, "raise ValueError('EXPECTED')", 1)?;
    let details = app.details(&workflow, &execution, 1)?;
    assert_eq!(details["status"], "failed");
    assert_eq!(details["error"]["code"], "execution_failed");
    assert_eq!(details["error"]["message"], "EXPECTED");
    assert_eq!(execution["error"], details["error"]);
    assert!(details["traceback"].as_str().unwrap().contains("ValueError: EXPECTED"));
    let execution = app.execute(id, &workflow, "if :", 1)?;
    let details = app.details(&workflow, &execution, 1)?;
    assert_eq!(details["error"]["code"], "preparation_failed");
    assert!(details["traceback"].as_str().unwrap().contains("SyntaxError"));
    assert_eq!(app.execute(id, &workflow, "print('alive')", 0)?["status"], "succeeded");
    Ok(())
}

#[test]
fn long_work_streams_output_and_refuses_shutdown_and_overlap() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("long")?;
    let release = app.directory.join("release-long-work");
    let code = format!(
        "import time\nfrom pathlib import Path\nprint('BEGIN', flush=True)\nwhile not Path({}).exists():\n    time.sleep(0.05)\nprint('DONE')",
        json!(release.to_string_lossy().as_ref())
    );
    let execution = app.execute(id, &workflow, &code, 0)?;
    assert_eq!(execution["status"], "running");
    wait_until(Duration::from_secs(15), || {
        Ok(app.details(&workflow, &execution, 0)?["stdout"]
            .as_str()
            .unwrap()
            .contains("BEGIN"))
    })?;
    assert_eq!(app.call("stop", &[], 1)?["error"]["code"], "backend_busy");
    assert_eq!(
        app.execute(id, &workflow, "print('must not run')", 1)?["error"]["code"],
        "instance_busy"
    );
    fs::write(&release, b"release")?;
    wait_until(Duration::from_secs(15), || {
        Ok(app.details(&workflow, &execution, 0)?["status"] == "succeeded")
    })?;
    assert_eq!(app.details(&workflow, &execution, 0)?["stdout"], "BEGIN\nDONE\n");
    let next = app.execute(id, &workflow, "print('NEXT')", 0)?;
    assert_eq!(next["status"], "succeeded");
    Ok(())
}

#[test]
fn execution_lookup_distinguishes_missing_records_from_storage_failures() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let lookup = |workflow: &str| app.call("execution", &["--workflow-id", workflow, "--execution-id", "0001"], 1);
    assert_eq!(lookup("missing")?["error"]["code"], "workflow_not_found");
    let workflow = app.workflow("empty")?;
    assert_eq!(lookup(&workflow)?["error"]["code"], "execution_not_found");
    let folder = app.directory.join("workflows");
    // A directory is deterministically unreadable as a record, without changing OS permissions.
    fs::create_dir(folder.join("unreadable.json"))?;
    let failure = &lookup("unreadable")?["error"];
    assert_eq!(failure["code"], "workflow_unreadable", "{failure}");
    assert!(
        failure["message"].as_str().unwrap().contains("unreadable.json"),
        "{failure}"
    );
    Ok(())
}
