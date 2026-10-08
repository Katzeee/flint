use crate::support::*;
use anyhow::Result;
use std::{fs, time::Duration};

#[test]
fn backend_reports_rejected_registrations_before_closing_the_connection() -> Result<()> {
    use flint_contracts::protocol::{
        envelope, envelope::Payload, framed, read_envelope, RegisterExecutionChannel,
        RegisterInstance,
    };
    use futures_util::SinkExt;

    let app = App::new();
    app.call("start", &[], 0)?;
    tokio::runtime::Runtime::new()?.block_on(async {
        for (request_id, request) in [
            (
                "invalid-bridge",
                Payload::RegisterInstance(RegisterInstance {
                    pid: 0,
                    bridge_id: "invalid-pid".into(),
                    name_hint: "test".into(),
                    instance_name: "Rejected Bridge".into(),
                    instance_type: "test".into(),
                    ..Default::default()
                }),
            ),
            (
                "unregistered-channel",
                Payload::RegisterExecutionChannel(RegisterExecutionChannel {
                    instance_id: "absent".into(),
                    pid: std::process::id(),
                    session_token: "unregistered".into(),
                }),
            ),
        ] {
            tokio::time::timeout(Duration::from_secs(5), async {
                let socket = tokio::net::TcpStream::connect(("127.0.0.1", app.bridge_port)).await?;
                let mut wire = framed(socket);
                wire.send(envelope(request_id.into(), request)).await?;
                let response = read_envelope(&mut wire).await?;
                assert_eq!(response.request_id, request_id);
                let Some(Payload::Failure(failure)) = response.payload else {
                    anyhow::bail!("{request_id}: expected a registration rejection");
                };
                assert_eq!(failure.code, "registration_rejected", "{request_id}");
                assert!(!failure.message.trim().is_empty(), "{request_id}");
                assert_eq!(
                    read_envelope(&mut wire).await.unwrap_err().kind(),
                    std::io::ErrorKind::UnexpectedEof,
                    "{request_id}: close after reporting refusal"
                );
                Ok::<_, anyhow::Error>(())
            })
            .await??;
        }
        Ok::<_, anyhow::Error>(())
    })?;
    assert_eq!(
        app.call("instances", &[], 0)?["instances"],
        serde_json::json!([])
    );
    Ok(())
}

#[test]
fn backend_lifecycle_preserves_python_host_and_records() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    assert_eq!(host.report["reused"], true);
    let instance = app.await_instance("standalone_python", None)?;
    assert_eq!(instance["pid"], host.report["pid"]);
    let id = instance["instance_id"].as_str().unwrap();
    let workflow = app.workflow("Unicode 场景")?;
    let file_code = "answer = 42\nprint('中文😀', answer)";
    let source = app.directory.join("example.py");
    fs::write(&source, file_code)?;
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
    assert_eq!(execution["status"], "succeeded");
    let stored = app.details(&workflow, &execution, 0)?;
    assert_eq!(stored["code"], file_code);
    assert_eq!(stored["stdout"], "中文😀 42\n");
    app.call("restart", &[], 0)?;
    let reconnected = app.await_instance("standalone_python", Some(id))?;
    assert_eq!(app.details(&workflow, &execution, 0)?, stored);
    let stdin_code = "print(answer)";
    let next = app.input(
        "exec",
        &[
            "--instance-id",
            reconnected["instance_id"].as_str().unwrap(),
            "--workflow-id",
            &workflow,
            "--stdin",
        ],
        0,
        Some(stdin_code),
    )?;
    let next_stored = app.details(&workflow, &next, 0)?;
    assert_eq!(next_stored["code"], stdin_code);
    assert_eq!(next_stored["stdout"], "42\n");
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
    let started = app.directory.join("execution-started");
    let release = app.directory.join("release-execution");
    let code = format!(
        "import time\nfrom pathlib import Path\nPath({}).touch()\nwhile not Path({}).exists():\n    time.sleep(0.05)\nwith open({}, 'a', newline='') as stream:\n    stream.write('once\\n')",
        serde_json::to_string(started.to_str().unwrap())?,
        serde_json::to_string(release.to_str().unwrap())?,
        serde_json::to_string(marker.to_str().unwrap())?
    );
    let execution = app.execute(id, &workflow, &code, 0)?;
    assert_eq!(execution["status"], "running");
    wait_until(Duration::from_secs(10), || Ok(started.exists()))?;
    host.drop_execution_connection()?;
    let reconnected = app.await_instance("standalone_python", Some(id))?;
    let lost = app.details(&workflow, &execution, 1)?;
    assert_eq!(lost["error"]["code"], "execution_disconnected");
    assert!(!lost["error"]["message"].as_str().unwrap().is_empty());
    assert!(!marker.exists());
    fs::write(release, b"release")?;
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
