use std::{path::Path, process::Command};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python_project = root.join("bridges/python");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for input in [
        "bridges/python/pyproject.toml",
        "bridges/python/uv.lock",
        "bridges/python/tools/package_bridge.py",
        "bridges/python/packages/bridge/pyproject.toml",
        "bridges/python/packages/bridge/src/flint_bridge",
        "bridges/python/packages/protocol/pyproject.toml",
        "bridges/python/packages/protocol/src/flint_protocol",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    let environment = out.join("python-env");
    let status = Command::new("uv")
        .current_dir(&python_project)
        .env("UV_PROJECT_ENVIRONMENT", &environment)
        .env_remove("VIRTUAL_ENV")
        .args([
            "sync",
            "--locked",
            "--no-default-groups",
            "--no-install-workspace",
            "--package",
            "flint-bridge",
            "--python",
            "3.13",
        ])
        .status()
        .expect("uv is required on PATH to build the Python Bridge");
    assert!(
        status.success(),
        "uv could not prepare the locked Bridge dependencies"
    );
    let python = environment.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    let status = Command::new(python)
        .current_dir(&python_project)
        .arg("-I")
        .arg(python_project.join("tools/package_bridge.py"))
        .arg(out.join("flint-bridge.zip"))
        .status()
        .expect("Cannot run the Bridge packager");
    assert!(status.success(), "Bridge packaging failed");
    tauri_build::build();
}
