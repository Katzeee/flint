use crate::support::*;
use anyhow::Result;
use serde_json::json;
use std::time::Duration;

#[cfg(windows)]
#[test]
fn shells_wait_for_cli_completion_and_receive_its_exit_code() -> Result<()> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::process::Command;

    let app = App::new();
    // A prior exit code exposes shells that return before the Flint command exits.
    let script = "$LASTEXITCODE = 91; & $env:FLINT_CLI_TEST_EXE --unknown 2>$null; [Console]::WriteLine('FLINT_EXIT:' + $LASTEXITCODE)";
    let encoded = STANDARD.encode(script.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
    let mut powershell = Command::new("powershell.exe");
    powershell
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
        .env("FLINT_CLI_TEST_EXE", &app.binary);
    let powershell_output = checked(&mut powershell, Duration::from_secs(10))?;

    // Feeding cmd without /c exercises its interactive command dispatch.
    let mut cmd = Command::new(std::env::var_os("ComSpec").expect("Windows command interpreter"));
    cmd.args(["/d", "/q", "/v:on"])
        .env("PROMPT", "$S")
        .env("FLINT_CLI_TEST_EXE", &app.binary);
    let cmd_output = run(
        &mut cmd,
        Duration::from_secs(10),
        Some(
            "cmd /d /c exit 91\r\n\"%FLINT_CLI_TEST_EXE%\" --unknown 2>nul\r\necho FLINT_EXIT:!errorlevel!\r\nexit /b 0\r\n",
        ),
    )?;
    assert!(cmd_output.status.success());
    assert!(
        powershell_output
            .stdout
            .lines()
            .any(|line| line.trim() == "FLINT_EXIT:2")
            && cmd_output.stdout.lines().any(|line| line.trim() == "FLINT_EXIT:2"),
        "PowerShell:\n{}\ncmd:\n{}",
        powershell_output.stdout,
        cmd_output.stdout
    );
    Ok(())
}

#[test]
fn concurrent_cli_calls_share_one_backend() -> Result<()> {
    let app = App::new();
    assert!(app.call("stop", &[], 0)?["stopped_pid"].is_null());
    let responses = std::thread::scope(|scope| {
        let calls: Vec<_> = (0..4).map(|_| scope.spawn(|| app.call("start", &[], 0))).collect();
        calls.into_iter().map(|t| t.join().unwrap()).collect::<Result<Vec<_>>>()
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
