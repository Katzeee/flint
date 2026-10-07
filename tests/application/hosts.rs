use crate::support::*;
use anyhow::Result;
use serde_json::Value;
use std::{process::Command, time::Duration};

#[cfg(windows)]
#[test]
fn host_info_infers_type_and_only_includes_a_requested_preview() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let fixture = directory.path().join("maya.exe");
    // A disposable recognized process without a window exercises headless host inspection.
    std::fs::copy(
        std::env::var_os("ComSpec").expect("Windows command interpreter"),
        &fixture,
    )?;
    let mut command = Command::new(&fixture);
    command
        .args(["/d", "/c", "set /p FLINT_HOST_FIXTURE_WAIT="])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    hidden(&mut command);
    let host = OwnedProcess(command.spawn()?);
    let pid = host.0.id().to_string();
    for preview in [false, true] {
        let mut command = Command::new(binary());
        command
            .args(["hosts", "info", "--pid", &pid, "--json"])
            .env("FLINT_TEST_ROOT", directory.path());
        if preview {
            command.arg("--preview");
        }
        let output = checked(&mut command, Duration::from_secs(10))?;
        let info: Value = serde_json::from_str(&output.stdout)?;
        assert_eq!(info["pid"], host.0.id());
        assert_eq!(info["host"], "maya");
        assert_eq!(info["executable"], fixture.to_string_lossy().as_ref());
        assert_eq!(info["window"], Value::Null);
        if preview {
            assert_eq!(info["preview"]["image"], Value::Null);
            assert!(info["preview"]["unavailable_reason"]
                .as_str()
                .is_some_and(|reason| !reason.is_empty()));
        } else {
            assert!(info.get("preview").is_none());
        }
    }
    assert!(!directory.path().join("workflows").exists());
    assert!(!directory.path().join("runtime").exists());
    Ok(())
}
