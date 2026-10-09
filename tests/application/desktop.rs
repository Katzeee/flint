use crate::support::*;
use anyhow::Result;
use std::{process::Stdio, time::Duration};

#[test]
fn desktop_survives_backend_stop_and_restart_and_reuses_its_window_process() -> Result<()> {
    let app = App::new();
    let mut command = app.launch_command();
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let mut desktop = OwnedProcess(command.spawn()?);
    wait_until(Duration::from_secs(30), || {
        anyhow::ensure!(desktop.0.try_wait()?.is_none(), "desktop exited during startup");
        Ok(app.call("status", &[], 0)?["ready"] == true)
    })?;
    let first = app.call("status", &[], 0)?["pid"].as_u64().unwrap();
    assert_ne!(first, u64::from(desktop.0.id()));
    app.call("stop", &[], 0)?;
    assert!(app.call("status", &[], 0)?.is_null());
    // A second launch returns to the existing GUI, which establishes the stopped backend again.
    checked(&mut app.command("gui"), Duration::from_secs(10))?;
    wait_until(Duration::from_secs(30), || {
        anyhow::ensure!(desktop.0.try_wait()?.is_none(), "desktop exited on relaunch");
        Ok(app.call("status", &[], 0)?["ready"] == true)
    })?;
    let reopened = app.call("status", &[], 0)?["pid"].as_u64().unwrap();
    assert_ne!(reopened, first);
    let restarted = app.call("restart", &[], 0)?["pid"].as_u64().unwrap();
    assert_ne!(restarted, reopened);
    assert_ne!(restarted, u64::from(desktop.0.id()));
    checked(&mut app.command("gui"), Duration::from_secs(10))?;
    assert!(desktop.0.try_wait()?.is_none());
    // Killing the desktop process leaves the backend running.
    drop(desktop);
    assert_eq!(app.call("status", &[], 0)?["pid"], restarted);
    Ok(())
}
