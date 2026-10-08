use super::conformance::{self, Driver};
use crate::support::*;
use anyhow::Result;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

/// Build `program` against the exported C# binding; returns the program's
/// assembly and the exported native core.
fn build_against_export(app: &App, program: &str) -> Result<(PathBuf, PathBuf)> {
    let bundle = app.directory.join("bundle");
    if !bundle.is_dir() {
        let archive = app.export_csharp()?;
        fs::create_dir_all(&bundle)?;
        checked(
            Command::new("tar")
                .arg("-xf")
                .arg(&archive)
                .arg("-C")
                .arg(&bundle),
            Duration::from_secs(15),
        )?;
    }
    let binding = bundle.join("NativeCore.cs");
    let native = bundle.join("flint_bridge_core.dll");
    anyhow::ensure!(
        binding.is_file() && native.is_file(),
        "Incomplete C# export"
    );

    let project = app.directory.join("csharp-host");
    fs::create_dir_all(&project)?;
    fs::write(
        project.join("FlintRuntimeHost.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Exe</OutputType>
  </PropertyGroup>
  <ItemGroup>
    <Compile Include="../bundle/*.cs" />
  </ItemGroup>
</Project>
"#,
    )?;
    fs::copy(fixture(program), project.join("Program.cs"))?;
    checked(
        Command::new("dotnet")
            .arg("build")
            .arg(project.join("FlintRuntimeHost.csproj"))
            .args(["--nologo", "--verbosity", "quiet"]),
        Duration::from_secs(90),
    )?;
    Ok((
        project.join("bin/Debug/net10.0/FlintRuntimeHost.dll"),
        native,
    ))
}

#[test]
#[ignore = "requires .NET 10; run `cargo xtask test csharp`"]
fn exported_zip_connects_and_executes_in_dotnet() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let (assembly, native) = build_against_export(&app, "csharp_host.cs")?;

    let ready = app.directory.join("csharp-ready.json");
    let stop = app.directory.join("csharp-stop");
    let mut command = Command::new("dotnet");
    command
        .arg(&assembly)
        .arg(&native)
        .arg(app.bridge_port.to_string())
        .arg(&ready)
        .arg(&stop)
        .current_dir(&app.directory)
        .stdin(Stdio::null())
        .stdout(fs::File::create(app.directory.join("csharp.stdout"))?)
        .stderr(fs::File::create(app.directory.join("csharp.stderr"))?);
    hidden(&mut command);
    let mut host = OwnedProcess(command.spawn()?);
    let mut report = None;
    wait_until(Duration::from_secs(25), || {
        report = fs::read(&ready)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if report.is_none() {
            anyhow::ensure!(
                host.0.try_wait()?.is_none(),
                "C# runtime exited: {}",
                fs::read_to_string(app.directory.join("csharp.stderr"))?
            );
        }
        Ok(report.is_some())
    })?;
    let report = report.unwrap();
    assert_eq!(report["pid"], host.0.id());
    let instance = app.await_instance("standalone_csharp", None)?;
    assert_eq!(instance["instance_id"], report["instance_id"]);
    let workflow = app.workflow("csharp-runtime-export")?;
    let execution = app.execute(
        instance["instance_id"].as_str().unwrap(),
        &workflow,
        "ping",
        0,
    )?;
    let mut detail = Value::Null;
    wait_until(Duration::from_secs(15), || {
        detail = app.details(&workflow, &execution, 0)?;
        Ok(detail["status"] == "succeeded")
    })?;
    assert_eq!(detail["stdout"], "CSHARP_ZIP_OK\n");
    let failed = app.execute(
        instance["instance_id"].as_str().unwrap(),
        &workflow,
        "unsupported",
        1,
    )?;
    let failure = app.details(&workflow, &failed, 1)?;
    assert_eq!(failure["status"], "failed");
    assert_eq!(failure["error"]["code"], "execution_failed");
    assert_eq!(failure["error"]["message"], "unsupported_test_command");
    assert!(failure["traceback"]
        .as_str()
        .unwrap()
        .contains("InvalidOperationException: unsupported_test_command"));
    assert_eq!(failed["error"], failure["error"]);
    let execution = app.execute(
        instance["instance_id"].as_str().unwrap(),
        &workflow,
        "async",
        0,
    )?;
    wait_until(Duration::from_secs(10), || {
        detail = app.details(&workflow, &execution, 0)?;
        Ok(detail["stdout"] == "BEGIN\0🙂\n")
    })?;
    assert_eq!(detail["status"], "running");
    let release = PathBuf::from(format!("{}.release", stop.display()));
    let cleaned = PathBuf::from(format!("{}.cleaned", release.display()));
    assert!(!cleaned.exists());
    fs::write(&release, b"release")?;
    wait_until(Duration::from_secs(10), || {
        detail = app.details(&workflow, &execution, 0)?;
        Ok(detail["status"] == "succeeded")
    })?;
    assert_eq!(detail["stderr"], "异步完成\n");
    assert!(cleaned.exists());
    fs::write(stop, b"stop")?;
    Ok(())
}

#[test]
#[ignore = "requires .NET 10; run `cargo xtask test csharp`"]
fn exported_zip_conforms_to_the_runtime_scenarios() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let (assembly, native) = build_against_export(&app, "conformance_driver.cs")?;
    let mut command = Command::new("dotnet");
    command
        .arg(assembly)
        .arg(native)
        .current_dir(&app.directory);
    conformance::verify(
        &app,
        "standalone_csharp",
        Driver::start(command)?,
        "held",
        "",
    )
}

#[test]
#[ignore = "requires .NET 10; run `cargo xtask test csharp`"]
fn exported_host_entry_preserves_the_connection_contract() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let (assembly, native) = build_against_export(&app, "csharp_integration_driver.cs")?;
    let mut command = Command::new("dotnet");
    command
        .arg(assembly)
        .arg(native)
        .current_dir(&app.directory);
    conformance::verify_host_entry(&app, "standalone_csharp", Driver::start(command)?)
}
