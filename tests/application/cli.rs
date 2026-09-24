use crate::support::*;
use anyhow::Result;
use serde_json::json;
use std::{process::Command, time::Duration};

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
