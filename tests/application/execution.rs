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
    assert!(details["traceback"]
        .as_str()
        .unwrap()
        .contains("ValueError: EXPECTED"));
    assert_eq!(
        app.execute(id, &workflow, "print('alive')", 0)?["status"],
        "succeeded"
    );
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
    assert!(app.details(&workflow, &execution, 0)?["stdout"]
        .as_str()
        .unwrap()
        .contains("BEGIN"));
    assert_eq!(app.call("stop", &[], 1)?["error_code"], "backend_busy");
    assert_eq!(
        app.execute(id, &workflow, "print('must not run')", 1)?["error_code"],
        "instance_busy"
    );
    fs::write(&release, b"release")?;
    wait_until(Duration::from_secs(15), || {
        Ok(app.details(&workflow, &execution, 0)?["status"] == "succeeded")
    })?;
    assert_eq!(
        app.details(&workflow, &execution, 0)?["stdout"],
        "BEGIN\nDONE\n"
    );
    let next = app.execute(id, &workflow, "print('NEXT')", 0)?;
    assert_eq!(next["status"], "succeeded");
    Ok(())
}

#[test]
fn file_and_stdin_sources_are_preserved() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("sources")?;
    let source = app.directory.join("example.py");
    fs::write(&source, "print('FROM_FILE')")?;
    let execution = app.call(
        "exec",
        &[
            "--instance-id",
            id,
            "--workflow-id",
            &workflow,
            "--file",
            source.to_str().unwrap(),
        ],
        0,
    )?;
    let details = app.details(&workflow, &execution, 0)?;
    assert_eq!(details["code"], "print('FROM_FILE')");
    assert_eq!(details["stdout"], "FROM_FILE\n");
    let execution = app.input(
        "exec",
        &["--instance-id", id, "--workflow-id", &workflow, "--stdin"],
        0,
        Some("print('FROM_STDIN')"),
    )?;
    assert_eq!(
        app.details(&workflow, &execution, 0)?["stdout"],
        "FROM_STDIN\n"
    );
    Ok(())
}
