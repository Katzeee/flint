use std::{fs, path::Path, process::Command};

fn watch_frontend_sources(directory: &Path) {
    for entry in fs::read_dir(directory).expect("Cannot read frontend source directory") {
        let entry = entry.expect("Cannot read frontend source entry");
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !matches!(
                name.to_str(),
                Some("dist" | "build" | "node_modules" | ".git")
            ) {
                watch_frontend_sources(&path);
            }
        } else if name != "generated.ts" {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn package(root: &Path, script: &str, output: &Path, native: &Path) {
    let status = Command::new("uv")
        .current_dir(root.join("bridges/python"))
        .args(["run", "--no-project", "--python", ">=3.11", "python"])
        .arg("-I")
        .arg(root.join(script))
        .arg(output)
        .arg("--native")
        .arg(native)
        .status()
        .expect("uv is required on PATH to run Bridge packagers");
    assert!(
        status.success(),
        "Bridge packaging failed: {script}; packagers need Python >=3.11 discoverable by uv"
    );
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    for input in [
        "apps/flint/index.html",
        "apps/flint/vite.config.ts",
        "apps/flint/package.json",
        "apps/flint/package-lock.json",
        "apps/flint/cairn/package.json",
        "apps/flint/cairn/tsconfig.json",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    for directory in [
        "apps/flint/src",
        "apps/flint/public",
        "apps/flint/cairn/packages",
        "apps/flint/cairn/scripts",
    ] {
        watch_frontend_sources(&root.join(directory));
    }
    for input in [
        "Cargo.toml",
        "Cargo.lock",
        "bridges/python/pyproject.toml",
        "crates/flint-protocol/Cargo.toml",
        "crates/flint-protocol/src",
        "crates/flint-bridge-core/Cargo.toml",
        "crates/flint-bridge-core/src",
        "bridges/python/tools/package_bridge.py",
        "bridges/python/packages/bridge/src/flint_bridge",
        "bridges/python/hosts/blender",
        "bridges/python/hosts/maya",
        "bridges/python/hosts/max",
        "bridges/dotnet/tools/package_csharp.py",
        "bridges/dotnet/hosts/unity",
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
    package(
        &root,
        "bridges/python/tools/package_bridge.py",
        &out.join("flint-python.zip"),
        &native,
    );
    package(
        &root,
        "bridges/python/hosts/blender/package.py",
        &out.join("flint-blender.zip"),
        &native,
    );
    package(
        &root,
        "bridges/python/hosts/maya/package.py",
        &out.join("flint-maya.zip"),
        &native,
    );
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        package(
            &root,
            "bridges/python/hosts/max/package.py",
            &out.join("flint-max.zip"),
            &native,
        );
        package(
            &root,
            "bridges/dotnet/tools/package_csharp.py",
            &out.join("flint-csharp.zip"),
            &native,
        );
        if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") {
            package(
                &root,
                "bridges/dotnet/hosts/unity/upm/package.py",
                &out.join("flint-unity.tgz"),
                &native,
            );
        }
    }
    let frontend = root.join("apps/flint");
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .current_dir(&frontend)
        .args(["run", "build"])
        .status()
        .expect(
            "Node.js and npm are required to build the Flint desktop UI; see docs/development.md",
        );
    assert!(
        status.success(),
        "Flint desktop UI build failed; run npm ci in apps/flint"
    );
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("Tauri build failed");
}
