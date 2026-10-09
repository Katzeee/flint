use super::conformance::{self, Driver};
use crate::support::*;
use anyhow::Result;
use std::process::Command;

#[test]
#[ignore = "requires Python; run `cargo xtask test python`"]
fn exported_zip_conforms_to_the_runtime_scenarios() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let bundle = app.export("python")?;
    let mut command = Command::new(python());
    command
        .args(["-I", "-S", "-X", "utf8"])
        .arg(fixture("conformance_driver.py"))
        .arg(bundle)
        .current_dir(&app.directory)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    conformance::verify(
        &app,
        "standalone_python",
        Driver::start(command)?,
        "print('PYTHON_ZIP_OK')",
        "PYTHON_ZIP_OK\n",
    )
}

#[test]
#[ignore = "requires Python; run `cargo xtask test python`"]
fn exported_host_entry_preserves_the_connection_contract() -> Result<()> {
    let app = App::new();
    app.call("start", &[], 0)?;
    let bundle = app.export("python")?;
    let mut command = Command::new(python());
    command
        .args(["-I", "-S", "-X", "utf8"])
        .arg(fixture("python_integration_driver.py"))
        .arg(bundle)
        .current_dir(&app.directory)
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONHOME");
    conformance::verify_host_entry(&app, "standalone_python", Driver::start(command)?)
}
