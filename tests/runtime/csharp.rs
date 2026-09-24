use crate::support::*;
use anyhow::Result;
use serde_json::Value;
use std::{
    fs,
    process::{Command, Stdio},
    time::Duration,
};

#[test]
#[ignore = "requires .NET 10; run `cargo xtask test csharp`"]
fn exported_zip_connects_and_executes_in_dotnet() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let archive = app.export_csharp()?;
    let bundle = app.directory.join("bundle");
    fs::create_dir_all(&bundle)?;
    checked(
        Command::new("tar")
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(&bundle),
        Duration::from_secs(15),
    )?;
    let binding = bundle.join("NativeBridge.cs");
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
    <Compile Include="../bundle/NativeBridge.cs" Link="NativeBridge.cs" />
  </ItemGroup>
</Project>
"#,
    )?;
    fs::copy(fixture("csharp_host.cs"), project.join("Program.cs"))?;
    checked(
        Command::new("dotnet")
            .arg("build")
            .arg(project.join("FlintRuntimeHost.csproj"))
            .args(["--nologo", "--verbosity", "quiet"]),
        Duration::from_secs(90),
    )?;

    let ready = app.directory.join("csharp-ready.json");
    let stop = app.directory.join("csharp-stop");
    let mut command = Command::new("dotnet");
    command
        .arg(project.join("bin/Debug/net10.0/FlintRuntimeHost.dll"))
        .arg(&native)
        .arg(app.registry_port.to_string())
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
    let instance = app.await_instance("csharp", None)?;
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
    fs::write(stop, b"stop")?;
    Ok(())
}
