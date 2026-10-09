use crate::support::*;
use anyhow::Result;
use serde_json::json;
use std::time::Duration;

#[test]
fn concurrent_cli_calls_share_one_backend() -> Result<()> {
    let app = App::new();
    assert!(app.call("stop", &[], 0)?["stopped_pid"].is_null());
    let responses = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| app.call("start", &[], 0)))
            .collect();
        calls
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Result<Vec<_>>>()
    })?;
    for response in &responses {
        assert_eq!(response["pid"], responses[0]["pid"]);
    }
    assert_eq!(app.call("start", &[], 0)?["pid"], responses[0]["pid"]);
    let duplicate = run(&mut app.command("serve"), Duration::from_secs(10), None)?;
    assert!(!duplicate.status.success());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&duplicate.stdout)?["error"]["code"],
        "backend_locked"
    );
    assert_eq!(app.call("status", &[], 0)?["pid"], responses[0]["pid"]);
    assert_ne!(app.call("restart", &[], 0)?["pid"], responses[0]["pid"]);
    assert_eq!(app.call("instances", &[], 0)?["instances"], json!([]));
    Ok(())
}

#[test]
fn input_loading_and_host_discovery_do_not_start_backend() -> Result<()> {
    let app = App::new();
    assert!(app.call("status", &[], 0)?.is_null());
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
        )?["error"]["code"],
        "command_failed"
    );
    let output = checked(&mut app.command("hosts"), Duration::from_secs(10))?;
    let discovery: serde_json::Value = serde_json::from_str(&output.stdout)?;
    assert!(discovery["hosts"].is_array());
    assert!(!app.directory.join("workflows").exists());
    assert!(!app.directory.join("runtime").exists());
    Ok(())
}
