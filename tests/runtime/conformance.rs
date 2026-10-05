//! One set of scenarios that every runtime binding must answer identically.
//!
//! Each runtime supplies a driver that reads one JSON command per line from
//! stdin and answers with one JSON line, reporting exactly what its binding
//! produced. The scenarios and their assertions live here, so a new runtime
//! conforms by passing them through its own driver.

use crate::support::*;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    process::{ChildStdin, ChildStdout, Command, Stdio},
    time::Duration,
};

pub struct Driver {
    _process: OwnedProcess,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
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
        Ok(Self {
            _process: process,
            input,
            output,
        })
    }

    fn call(&mut self, command: Value) -> Result<Value> {
        writeln!(self.input, "{command}")?;
        self.input.flush()?;
        let mut line = String::new();
        self.output.read_line(&mut line)?;
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

/// Runs every scenario through `driver`, a binding for `host` connected to `app`.
pub fn verify(app: &App, host: &str, mut driver: Driver) -> Result<()> {
    let config = |port: u16| {
        json!({"host": host, "address": "127.0.0.1", "port": port,
               "name": "conformance", "runtime_version": "conformance"})
    };

    // Creation failures carry a kind beside the message.
    let invalid = driver.call(json!({"op": "create", "config": {}}))?;
    assert_eq!(
        invalid["error"]["kind"], "invalid_configuration",
        "{invalid}"
    );
    assert!(
        invalid["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("Invalid Bridge configuration: missing field `host`"),
        "{invalid}"
    );
    let missing = app.directory.join("missing").join("flint_bridge_core.dll");
    let unavailable = driver.call(json!({"op": "create", "library": missing,
                                         "config": config(app.bridge_port)}))?;
    assert_eq!(
        unavailable["error"]["kind"], "library_unavailable",
        "{unavailable}"
    );

    // A connected Bridge's snapshot has exactly this shape.
    let created = driver.call(json!({"op": "create", "config": config(app.bridge_port)}))?;
    assert_eq!(created, json!({"created": true}));
    let connected = driver.status_until("connected")?;
    let instance = connected["connection"]["instance_id"].clone();
    assert!(instance.as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(
        connected,
        json!({"connection": {"state": "connected", "instance_id": instance},
               "busy": false, "settings": settings(app.bridge_port, "conformance")})
    );

    let claimed = driver.call(json!({"op": "create", "config": config(app.bridge_port)}))?;
    assert_eq!(claimed["error"]["kind"], "claimed", "{claimed}");
    assert_eq!(
        claimed["error"]["message"],
        format!(
            "Another Bridge already owns this process: host={host}, \
             runtime_version=conformance, bridge_version={}",
            env!("CARGO_PKG_VERSION")
        )
    );

    // A refused operation leaves the state as it was.
    let refused = driver.call(json!({"op": "apply", "settings": settings(0, "conformance")}))?;
    assert_eq!(refused, json!({"rejected": "invalid_settings"}));
    assert_eq!(driver.call(json!({"op": "status"}))?, connected);

    // `exec` waits for the result, which the driver holds until told to report.
    let workflow = app.workflow("runtime-conformance")?;
    let execution = std::thread::scope(|scope| -> Result<Value> {
        let submitted =
            scope.spawn(|| app.execute(instance.as_str().unwrap(), &workflow, "held", 0));
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

    // A connection that cannot be made explains itself inside its state.
    let unreachable = free_port();
    let moved = driver.call(json!({"op": "apply", "settings": settings(unreachable, "moved")}))?;
    assert_eq!(moved, json!({"applied": true}));
    let retrying = driver.status_until("retrying")?;
    let obstacle = &retrying["connection"]["obstacle"];
    assert_eq!(obstacle["kind"], "unreachable", "{retrying}");
    assert!(obstacle["message"].as_str().is_some_and(|m| !m.is_empty()));
    assert_eq!(
        retrying["connection"].as_object().unwrap().len(),
        2,
        "{retrying}"
    );
    assert_eq!(retrying["settings"], settings(unreachable, "moved"));

    assert_eq!(
        driver.call(json!({"op": "reconnect"}))?,
        json!({"reconnected": true})
    );
    assert_eq!(
        driver.call(json!({"op": "close"}))?,
        json!({"closed": true})
    );
    Ok(())
}
