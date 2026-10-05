use super::conformance::{self, Driver};
use crate::support::*;
use anyhow::Result;
use std::process::Command;

#[test]
#[ignore = "requires Python; run `cargo xtask test python`"]
fn exported_zip_connects_and_executes_in_python() -> Result<()> {
    let app = App::new();
    let host = PythonHost::start(&app, &python())?;
    let archive = app.directory.join("flint-python.zip");
    let imported = host.report["module_file"].as_str().unwrap();
    assert!(
        imported
            .to_lowercase()
            .starts_with(&archive.to_string_lossy().to_lowercase()),
        "Python loaded the Bridge from {imported}, not {}",
        archive.display()
    );
    let instance = app.await_instance("python", None)?;
    assert_eq!(instance["pid"], host.report["pid"]);
    let workflow = app.workflow("python-runtime-export")?;
    let execution = app.execute(
        instance["instance_id"].as_str().unwrap(),
        &workflow,
        "print('PYTHON_ZIP_OK')",
        0,
    )?;
    assert_eq!(execution["status"], "succeeded");
    assert_eq!(
        app.details(&workflow, &execution, 0)?["stdout"],
        "PYTHON_ZIP_OK\n"
    );
    Ok(())
}

#[test]
#[ignore = "requires Python; run `cargo xtask test python`"]
fn exported_zip_conforms_to_the_runtime_scenarios() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let bundle = app.export()?;
    let mut command = Command::new(python());
    command
        .args(["-I", "-S", "-X", "utf8"])
        .arg(fixture("conformance_driver.py"))
        .arg(bundle)
        .current_dir(&app.directory)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    conformance::verify(&app, "python", Driver::start(command)?)
}
