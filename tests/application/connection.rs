use crate::support::*;
use anyhow::Result;
use std::{fs, time::Duration};

#[test]
fn backend_lifecycle_preserves_python_host_and_records() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    assert_eq!(host.report["reused"], true);
    let instance = app.await_instance("python", None)?;
    assert_eq!(instance["pid"], host.report["pid"]);
    let id = instance["instance_id"].as_str().unwrap();
    let workflow = app.workflow("Unicode 场景")?;
    let execution = app.execute(id, &workflow, "answer = 42\nprint('中文😀', answer)", 0)?;
    assert_eq!(execution["status"], "succeeded");
    let stored = app.details(&workflow, &execution, 0)?;
    assert_eq!(stored["stdout"], "中文😀 42\n");
    app.call("restart", &[], 0)?;
    let reconnected = app.await_instance("python", Some(id))?;
    assert_eq!(app.details(&workflow, &execution, 0)?, stored);
    let next = app.execute(
        reconnected["instance_id"].as_str().unwrap(),
        &workflow,
        "print(answer)",
        0,
    )?;
    assert_eq!(app.details(&workflow, &next, 0)?["stdout"], "42\n");
    assert_eq!(app.call("stop", &[], 0)?["stopped"], true);
    fs::write(app.directory.join("ping-host"), b"ping")?;
    let alive = app.directory.join("host-alive");
    wait_until(Duration::from_secs(3), || Ok(alive.exists()))?;
    assert_eq!(fs::read_to_string(alive)?, host.report["pid"].to_string());
    Ok(())
}

#[test]
fn a_lost_connection_does_not_replay_running_code() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("disconnected")?;
    let marker = app.directory.join("executed.txt");
    let code = format!(
        "import time\ntime.sleep(7)\nwith open({}, 'a', newline='') as stream:\n    stream.write('once\\n')",
        serde_json::to_string(marker.to_str().unwrap())?
    );
    let execution = app.execute(id, &workflow, &code, 0)?;
    assert_eq!(execution["status"], "running");
    host.drop_execution_connection()?;
    let reconnected = app.await_instance("python", Some(id))?;
    let lost = app.details(&workflow, &execution, 1)?;
    assert!(lost["error"].as_str().unwrap().contains("unknown"));
    wait_until(Duration::from_secs(10), || Ok(marker.exists()))?;
    assert_eq!(fs::read_to_string(&marker)?, "once\n");
    wait_until(Duration::from_secs(10), || {
        Ok(host.directory.join("bridge-idle-after-drop").exists())
    })?;
    let after = app.execute(
        reconnected["instance_id"].as_str().unwrap(),
        &workflow,
        "print('AFTER_RECONNECT')",
        0,
    )?;
    assert_eq!(after["status"], "succeeded");
    assert_eq!(fs::read_to_string(marker)?, "once\n");
    Ok(())
}
