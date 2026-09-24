use std::{path::Path, process::Command};

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let python_project = root.join("bridges/python");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for input in [
        "Cargo.lock",
        "crates/flint-protocol/Cargo.toml",
        "crates/flint-protocol/src",
        "crates/flint-bridge-core/Cargo.toml",
        "crates/flint-bridge-core/src",
        "bridges/python/tools/package_bridge.py",
        "bridges/python/packages/bridge/pyproject.toml",
        "bridges/python/packages/bridge/src/flint_bridge",
        "bridges/dotnet/unity/EditorBridge.cs",
        "bridges/dotnet/src/Flint.Bridge/NativeBridge.cs",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    let target = std::env::var("TARGET").expect("Cargo target triple");
    let native_target = out.join("native-target");
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&root)
        .args([
            "build",
            "--locked",
            "--release",
            "-p",
            "flint-bridge-core",
            "--target",
        ])
        .arg(&target)
        .arg("--target-dir")
        .arg(&native_target)
        .status()
        .expect("Cannot build the native Bridge core");
    assert!(status.success(), "Native Bridge core build failed");
    let library = match std::env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "windows" => "flint_bridge_core.dll",
        "macos" => "libflint_bridge_core.dylib",
        _ => "libflint_bridge_core.so",
    };
    let native = native_target.join(target).join("release").join(library);
    let status = Command::new("uv")
        .current_dir(&python_project)
        .args(["run", "--no-project", "--python", "3.13", "python"])
        .arg("-I")
        .arg(python_project.join("tools/package_bridge.py"))
        .arg(out.join("flint-bridge.zip"))
        .arg("--native")
        .arg(&native)
        .status()
        .expect("uv is required on PATH to run the Bridge packager");
    assert!(status.success(), "Bridge packaging failed");
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("Tauri build failed");
}
