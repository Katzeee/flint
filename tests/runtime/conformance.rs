//! Shared connection contracts exercised through bindings and platform managers.
//!
//! Each runtime supplies a driver that reads one JSON command per line from
//! stdin and answers with one JSON line, reporting production results. Binding
//! creation and event delivery are checked at the binding boundary; settings and
//! resource release also run through managers with controlled host capabilities.

use crate::support::*;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{ChildStdin, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

pub struct Driver {
    _process: OwnedProcess,
    input: ChildStdin,
    output: mpsc::Receiver<std::io::Result<String>>,
}

impl Driver {
    pub fn start(mut command: Command) -> Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        hidden(&mut command);
        let mut process = OwnedProcess(command.spawn()?);
        let input = process.0.stdin.take().unwrap();
        let output = BufReader::new(process.0.stdout.take().unwrap());
        let (send, receive) = mpsc::channel();
        std::thread::spawn(move || {
            for line in output.lines() {
                if send.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            _process: process,
            input,
            output: receive,
        })
    }

    fn call(&mut self, command: Value) -> Result<Value> {
        self.call_with_timeout(command, Duration::from_secs(30))
    }

    fn call_with_timeout(&mut self, command: Value, timeout: Duration) -> Result<Value> {
        writeln!(self.input, "{command}")?;
        self.input.flush()?;
        let line = self
            .output
            .recv_timeout(timeout)
            .with_context(|| format!("driver did not answer {command} within {timeout:?}"))??;
        serde_json::from_str(&line).with_context(|| format!("{command} answered {line:?}"))
    }

    fn status_until(&mut self, state: &str) -> Result<Value> {
        let mut status = Value::Null;
        wait_until(Duration::from_secs(20), || {
            status = self.call(json!({"op": "status"}))?;
            Ok(status["connection"]["state"] == state)
        })
        .with_context(|| format!("never reached {state}; last status {status}"))?;
        Ok(status)
    }
}

fn settings(port: u16, name: &str) -> Value {
    json!({"address": "127.0.0.1", "port": port, "name": name, "enabled": true})
}

fn config(host: &str, port: u16) -> Value {
    json!({"host": host, "address": "127.0.0.1", "port": port,
           "name": "场景 🌍", "runtime_version": "conformance"})
}

/// Verify execution callbacks, settings and diagnostics through the exported binding.
pub fn verify(app: &App, host: &str, mut driver: Driver, code: &str, stdout: &str) -> Result<()> {
    let created = driver.call(json!({"op": "create", "config": config(host, app.bridge_port)}))?;
    assert_eq!(created, json!({"created": true}));
    let connected = driver.status_until("connected")?;
    let instance = connected["connection"]["instance_id"].clone();
    assert!(instance.as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(connected["busy"], false);
    assert_eq!(connected["settings"]["address"], "127.0.0.1");
    assert_eq!(connected["settings"]["port"], app.bridge_port);
    assert_eq!(connected["settings"]["name"], "场景 🌍");
    assert_eq!(connected["settings"]["enabled"], true);

    // `exec` waits for the result, which the driver holds until told to report.
    let workflow = app.workflow("runtime-conformance")?;
    let execution = std::thread::scope(|scope| -> Result<Value> {
        let submitted = scope.spawn(|| app.execute(instance.as_str().unwrap(), &workflow, code, 0));
        let taken = driver.call(json!({"op": "take"}))?;
        assert!(taken["request_id"].is_string(), "{taken}");
        assert_eq!(driver.call(json!({"op": "status"}))?["busy"], true);
        let busy =
            driver.call(json!({"op": "apply", "settings": settings(free_port(), "moved")}))?;
        assert_eq!(busy, json!({"rejected": "busy"}));
        assert_eq!(
            driver.call(json!({"op": "finish"}))?,
            json!({"reported": true})
        );
        submitted.join().unwrap()
    })?;
    assert_eq!(execution["status"], "succeeded", "{execution}");
    assert_eq!(app.details(&workflow, &execution, 0)?["stdout"], stdout);

    verify_binding_settings(&mut driver)?;
    assert_eq!(
        driver.call(json!({"op": "close"}))?,
        json!({"closed": true})
    );
    Ok(())
}

/// Exercise the platform host manager through its real connect/attach entry.
pub fn verify_host_entry(app: &App, host: &str, mut driver: Driver) -> Result<()> {
    let options = config(host, app.bridge_port);
    assert_eq!(
        driver.call(json!({"op": "create", "config": options}))?,
        json!({"created": true})
    );
    driver.status_until("connected")?;

    // Reattaching applies settings through the real host entry, including when
    // registration must move to a different backend.
    let destination = App::new();
    destination.call("start", &[], 0)?;
    let attached = driver.call(
        json!({"op": "attach", "settings": settings(destination.bridge_port, "reattached")}),
    )?;
    let moved = destination.await_instance(host, None)?;
    assert_eq!(attached["instance_id"], moved["instance_id"]);
    assert_eq!(
        driver.call(
            json!({"op": "attach", "settings": settings(destination.bridge_port, "reattached")})
        )?,
        attached
    );
    driver.call(json!({"op": "attach", "settings": settings(app.bridge_port, "conformance")}))?;

    verify_invalid_settings(app, &mut driver)?;
    assert_eq!(
        driver.call(json!({"op": "close"}))?,
        json!({"closed": true})
    );
    verify_manager_lifecycle(app, host, &mut driver)
}

/// Wrappers must preserve explicitly supplied invalid values rather than defaulting them.
fn verify_invalid_settings(app: &App, driver: &mut Driver) -> Result<()> {
    let connected = driver.status_until("connected")?;
    for invalid in [settings(0, "conformance"), settings(app.bridge_port, "")] {
        assert_eq!(
            driver.call(json!({"op": "apply", "settings": invalid}))?,
            json!({"rejected": "invalid_settings"})
        );
        assert_eq!(driver.call(json!({"op": "status"}))?, connected);
    }
    Ok(())
}

/// Settings and native connection diagnostics survive the language boundary.
fn verify_binding_settings(driver: &mut Driver) -> Result<()> {
    let unreachable = free_port();
    let moved = driver.call(json!({"op": "apply", "settings": settings(unreachable, "新场景")}))?;
    assert_eq!(moved, json!({"applied": true}));
    let retrying = driver.status_until("retrying")?;
    let obstacle = &retrying["connection"]["obstacle"];
    assert_eq!(obstacle["kind"], "unreachable", "{retrying}");
    assert!(obstacle["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert_eq!(retrying["settings"], settings(unreachable, "新场景"));
    Ok(())
}

/// The driver controls the host executor and dispatch queue; the exported
/// manager, Bridge and native core make every lifecycle decision.
fn verify_manager_lifecycle(app: &App, host: &str, driver: &mut Driver) -> Result<()> {
    let options = config(host, app.bridge_port);
    let initial = driver.call(json!({"op": "probe"}))?;
    let created = initial["created"].as_u64().unwrap();
    let released = initial["released"].as_u64().unwrap();
    let missing = app.directory.join("missing-core.dll");
    let failed = driver.call(json!({"op": "create", "config": options, "library": missing}))?;
    assert_eq!(failed["error"]["kind"], "library_unavailable", "{failed}");
    let probe = driver.call(json!({"op": "probe"}))?;
    // Failed startup releases whatever execution resources it acquired.
    let acquired = probe["created"].as_u64().unwrap();
    let disposed = probe["released"].as_u64().unwrap();
    assert_eq!(acquired - created, disposed - released);
    let (created, released) = (acquired, disposed);
    assert!(driver.call(json!({"op": "status"}))?.is_null());

    // The host eventually drains a timed-out callback. It must do no work.
    driver.call(json!({"op": "attach_begin", "settings": settings(app.bridge_port, "conformance"), "timeout_ms": 0}))?;
    let timed_out = driver.call(json!({"op": "attach_result"}))?;
    assert!(
        timed_out["error"]["message"]
            .as_str()
            .unwrap()
            .contains("cancelled before starting"),
        "{timed_out}"
    );
    driver.call(json!({"op": "close"}))?;
    driver.call(json!({"op": "drain"}))?;
    assert_eq!(driver.call(json!({"op": "probe"}))?["created"], created);

    driver.call(json!({"op": "attach_begin", "settings": settings(app.bridge_port, "conformance"), "timeout_ms": 10000}))?;
    driver.call(json!({"op": "drain"}))?;
    let attached = driver.call(json!({"op": "attach_result"}))?;
    let connected = driver.status_until("connected")?;
    assert_eq!(
        attached["instance_id"],
        connected["connection"]["instance_id"]
    );
    let probe = driver.call(json!({"op": "probe"}))?;
    assert_eq!(probe["created"], created + 1);
    assert_eq!(probe["factory_thread"], probe["dispatch_thread"]);
    let mut disabled = settings(app.bridge_port, "paused");
    disabled["enabled"] = json!(false);
    driver.call(json!({"op": "apply", "settings": disabled}))?;
    driver.status_until("disabled")?;
    driver.call(json!({"op": "apply", "settings": settings(app.bridge_port, "conformance")}))?;
    let connected = driver.status_until("connected")?;
    assert_eq!(driver.call(json!({"op": "probe"}))?["created"], created + 1);

    let workflow = app.workflow("manager-lifecycle")?;
    let instance = connected["connection"]["instance_id"].as_str().unwrap();
    let before = driver.call(json!({"op": "probe"}))?;
    let execution = std::thread::scope(|scope| -> Result<Value> {
        // The backend loses the outcome, while the host execution remains held.
        let submitted = scope.spawn(|| app.execute(instance, &workflow, "held", 1));
        assert_eq!(
            driver.call(json!({"op": "wait_started"}))?,
            json!({"started": true})
        );
        assert_eq!(
            driver.call_with_timeout(json!({"op": "close"}), Duration::from_secs(2))?,
            json!({"closed": false})
        );
        let stopped = driver.call(json!({"op": "status"}))?;
        assert_eq!(stopped["connection"], json!({"state": "stopped"}));
        assert_eq!(stopped["busy"], true);
        assert_eq!(
            driver.call(json!({"op": "probe"}))?["released"],
            before["released"]
        );
        for command in [
            json!({"op": "create", "config": options}),
            json!({"op": "apply", "settings": settings(app.bridge_port, "changed")}),
            json!({"op": "reconnect"}),
            json!({"op": "attach", "settings": settings(app.bridge_port, "changed")}),
        ] {
            assert_eq!(driver.call(command)?, json!({"rejected": "stopped"}));
        }
        assert_eq!(
            driver.call(json!({"op": "claim", "config": options}))?["error"]["kind"],
            "claimed"
        );
        assert_eq!(
            driver.call(json!({"op": "probe"}))?["finished"],
            before["finished"]
        );
        driver.call(json!({"op": "release"}))?;
        wait_until(Duration::from_secs(5), || {
            Ok(driver.call(json!({"op": "status"}))?["busy"] == false)
        })?;
        wait_until(Duration::from_secs(5), || {
            Ok(driver.call(json!({"op": "close"}))?["closed"] == true)
        })?;
        submitted.join().unwrap()
    })?;
    assert_eq!(execution["status"], "failed", "{execution}");
    assert_eq!(
        driver.call(json!({"op": "close"}))?,
        json!({"closed": true})
    );
    let probe = driver.call(json!({"op": "probe"}))?;
    assert_eq!(probe["created"], created + 1);
    assert_eq!(probe["released"], released + 1);
    assert_eq!(
        probe["finished"].as_u64().unwrap(),
        before["finished"].as_u64().unwrap() + 1
    );

    driver.call(json!({"op": "create", "config": options}))?;
    assert_ne!(
        driver.status_until("connected")?["connection"]["instance_id"],
        instance
    );
    driver.call(json!({"op": "close"}))?;
    let probe = driver.call(json!({"op": "probe"}))?;
    assert_eq!(probe["created"], created + 2);
    assert_eq!(probe["released"], released + 2);
    Ok(())
}
