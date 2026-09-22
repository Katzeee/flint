mod support;
use anyhow::Result;
use serde_json::json;
use std::{env, fs, path::PathBuf, process::Command, time::Duration};
use support::*;

#[test]
fn concurrent_cli_calls_share_one_backend() -> Result<()> {
    let app = App::new();
    assert_eq!(app.call("stop", &[], 0)?["already_stopped"], true);
    let responses = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| app.call("status", &[], 0)))
            .collect();
        calls
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Result<Vec<_>>>()
    })?;
    for response in &responses {
        assert_eq!(response["backend_id"], responses[0]["backend_id"]);
    }
    assert_eq!(app.call("start", &[], 0)?["pid"], responses[0]["pid"]);
    assert_ne!(
        app.call("restart", &[], 0)?["backend_id"],
        responses[0]["backend_id"]
    );
    assert_eq!(app.call("instances", &[], 0)?["instances"], json!([]));
    Ok(())
}

#[test]
fn bridge_reconnects_without_losing_namespace_or_records() -> Result<()> {
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
    Ok(())
}

#[test]
fn python_errors_are_recorded_without_terminating_host() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("errors")?;
    for (source, diagnostic) in [
        ("raise ValueError('EXPECTED')", "ValueError"),
        ("if :", "SyntaxError"),
        ("raise SystemExit('EXPECTED')", "SystemExit"),
    ] {
        let execution = app.execute(id, &workflow, source, 1)?;
        let details = app.details(&workflow, &execution, 1)?;
        assert!(details["traceback"].as_str().unwrap().contains(diagnostic));
        assert_eq!(
            app.execute(id, &workflow, "print('alive')", 0)?["status"],
            "succeeded"
        );
    }
    Ok(())
}

#[test]
fn long_work_streams_output_and_refuses_shutdown_and_overlap() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("long")?;
    let execution = app.execute(
        id,
        &workflow,
        "import time\nprint('BEGIN', flush=True)\ntime.sleep(8)\nprint('DONE')",
        0,
    )?;
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
    wait_until(Duration::from_secs(15), || {
        Ok(app.details(&workflow, &execution, 0)?["status"] == "succeeded")
    })?;
    assert_eq!(
        app.details(&workflow, &execution, 0)?["stdout"],
        "BEGIN\nDONE\n"
    );
    Ok(())
}

#[test]
fn invalid_input_and_help_do_not_start_backend() -> Result<()> {
    let app = App::new();
    for argument in ["--help", "--version"] {
        checked(
            Command::new(&app.binary)
                .arg(argument)
                .env("FLINT_STATE_DIR", app.directory.join("state")),
            Duration::from_secs(10),
        )?;
    }
    assert_eq!(app.call("exec", &[], 2)?["error_code"], "invalid_arguments");
    assert_eq!(
        app.call(
            "exec",
            &[
                "--instance-id",
                "absent",
                "--workflow-id",
                "absent",
                "--file",
                "missing.py"
            ],
            1
        )?["error_code"],
        "command_failed"
    );
    assert!(!app
        .directory
        .join("state/runtime")
        .join(app.port.to_string())
        .join("backend.log")
        .exists());
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

#[test]
fn exported_bridge_contains_its_portable_dependencies() -> Result<()> {
    let app = App::new();
    let bundle = app.export()?;
    let mut archive = zip::ZipArchive::new(fs::File::open(bundle)?)?;
    for name in [
        "flint_bridge/__init__.py",
        "flint_protocol/v1/envelope_pb2.py",
        "google/protobuf/__init__.py",
        "licenses/protobuf.txt",
    ] {
        assert!(archive.by_name(name).is_ok(), "Missing {name}");
    }
    assert!(!archive.file_names().any(|n| n.starts_with("flint/server")));
    Ok(())
}

#[test]
#[ignore = "Set FLINT_PYTHON37 to a Python 3.7 interpreter"]
fn python37_loads_the_exported_bridge() -> Result<()> {
    let interpreter =
        PathBuf::from(env::var_os("FLINT_PYTHON37").expect("FLINT_PYTHON37 is required"));
    let app = App::new();
    let host = PythonHost::start(&app, &interpreter)?;
    assert_eq!(host.report["version"], "3.7");
    let execution = app.execute(
        host.report["instance_id"].as_str().unwrap(),
        &app.workflow("python37")?,
        "print('PY37_OK')",
        0,
    )?;
    assert_eq!(execution["status"], "succeeded");
    Ok(())
}

#[test]
fn a_lost_connection_does_not_replay_running_code() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let id = host.report["instance_id"].as_str().unwrap();
    let workflow = app.workflow("disconnected")?;
    let marker = app.directory.join("executed.txt");
    let code=format!("import time\ntime.sleep(7)\nwith open({}, 'a', newline='') as stream:\n    stream.write('once\\n')",serde_json::to_string(marker.to_str().unwrap())?);
    let execution = app.execute(id, &workflow, &code, 0)?;
    assert_eq!(execution["status"], "running");
    host.drop_execution_connection()?;
    let reconnected = app.await_instance("python", Some(id))?;
    let lost = app.details(&workflow, &execution, 1)?;
    assert!(lost["error"].as_str().unwrap().contains("unknown"));
    wait_until(Duration::from_secs(10), || Ok(marker.exists()))?;
    assert_eq!(fs::read_to_string(&marker)?, "once\n");
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
