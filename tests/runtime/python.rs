use crate::support::*;
use anyhow::Result;

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
    let diagnostic = app.execute(
        instance["instance_id"].as_str().unwrap(),
        &workflow,
        r#"from flint_bridge.connection.native import NativeCore
try:
    duplicate = NativeCore({"host": "contender", "address": "127.0.0.1", "port": 1,
                            "name": "duplicate", "runtime_version": "contender", "enabled": False})
except RuntimeError as error:
    print(str(error))
else:
    duplicate.close()
    raise AssertionError("A second Bridge owns this process")
"#,
        0,
    )?;
    assert_eq!(diagnostic["status"], "succeeded");
    let details = app.details(&workflow, &diagnostic, 0)?;
    let message = details["stdout"].as_str().unwrap();
    assert!(message.contains("another Bridge already owns this process"));
    assert!(message.contains("host=python"));
    assert!(message.contains("runtime_version="));
    assert!(message.contains(&format!("bridge_version={}", env!("CARGO_PKG_VERSION"))));
    Ok(())
}
