use std::{fs, path::Path, process::Command};

fn watch_sources(directory: &Path) {
    for entry in fs::read_dir(directory).expect("Cannot read source directory") {
        let entry = entry.expect("Cannot read source entry");
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if !matches!(
                name.to_str(),
                Some(
                    "dist"
                        | "build"
                        | "node_modules"
                        | ".git"
                        | "generated"
                        | "bin"
                        | "obj"
                        | "__pycache__"
                        | ".venv"
                        | ".pytest_cache"
                )
            ) {
                watch_sources(&path);
            }
        } else if name != "generated.ts" {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

fn package(root: &Path, script: &str, output: &Path, native: &Path, managed: Option<&Path>) {
    let mut command = Command::new("uv");
    command
        .current_dir(root.join("bridges"))
        .args(["run", "--no-project", "--python", ">=3.11", "python"])
        .arg("-I")
        .arg(root.join(script))
        .arg(output)
        .arg("--native")
        .arg(native);
    if let Some(assembly) = managed {
        command.arg("--managed").arg(assembly);
    }
    let status = command
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
    assert!(
        root.join("apps/flint/cairn/package.json").is_file()
            && root
                .join("apps/flint/node_modules/.package-lock.json")
                .is_file(),
        "Frontend dependencies are not prepared; run cargo xtask build from the repository root"
    );
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
        watch_sources(&root.join(directory));
    }
    for input in [
        "Cargo.toml",
        "Cargo.lock",
        "bridges/global.json",
        "bridges/pyproject.toml",
        "crates/flint-contracts/Cargo.toml",
        "crates/flint-contracts/src",
        "crates/flint-bridge-core/Cargo.toml",
        "crates/flint-bridge-core/src",
        "crates/flint-bridge-bootstrap/Cargo.toml",
        "crates/flint-bridge-bootstrap/src",
        "bridges/platforms/python/tools/package_bridge.py",
        "bridges/platforms/python/src/flint_bridge",
        "bridges/hosts/standalone_python",
        "bridges/hosts/blender",
        "bridges/hosts/maya",
        "bridges/hosts/max",
        "bridges/platforms/dotnet/tools/package_csharp.py",
        "bridges/hosts/unity",
        "bridges/platforms/dotnet/src/Flint.Bridge",
    ] {
        let path = root.join(input);
        if path.is_dir() {
            watch_sources(&path);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
        }
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
    let native = native_target.join(&target).join("release").join(library);
    // Build one Unity adapter for both the Editor package and external attach.
    let core = out.join("flint_bridge_core.dll");
    let unity_bridge = out.join("flint-unity.dll");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        std::fs::copy(&native, &core).expect("Cannot stage the native core for attach");
        let build_dir = out.join("unity-bridge-build");
        let status = Command::new("dotnet")
            .current_dir(root.join("bridges/hosts/unity"))
            .args([
                "build",
                "-c",
                "Release",
                "-p:RestoreLockedMode=true",
                "--nologo",
                "-v",
                "quiet",
                "-o",
            ])
            .arg(&build_dir)
            .status()
            .expect("dotnet (from global.json) is required to build the Unity Bridge assembly");
        assert!(status.success(), "Unity Bridge assembly build failed");
        std::fs::copy(build_dir.join("Flint.Unity.dll"), &unity_bridge)
            .expect("Cannot stage the Unity Bridge assembly");
    } else {
        std::fs::write(&core, []).expect("Cannot stage the core placeholder");
        std::fs::write(&unity_bridge, []).expect("Cannot stage the Unity Bridge placeholder");
    }

    package(
        &root,
        "bridges/platforms/python/tools/package_bridge.py",
        &out.join("flint-python.zip"),
        &native,
        None,
    );
    package(
        &root,
        "bridges/hosts/blender/package.py",
        &out.join("flint-blender.zip"),
        &native,
        None,
    );
    package(
        &root,
        "bridges/hosts/maya/package.py",
        &out.join("flint-maya.zip"),
        &native,
        None,
    );
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        package(
            &root,
            "bridges/hosts/max/package.py",
            &out.join("flint-max.zip"),
            &native,
            None,
        );
        package(
            &root,
            "bridges/platforms/dotnet/tools/package_csharp.py",
            &out.join("flint-csharp.zip"),
            &native,
            None,
        );
        if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64") {
            package(
                &root,
                "bridges/hosts/unity/upm/package.py",
                &out.join("flint-unity.tgz"),
                &native,
                Some(&unity_bridge),
            );
        }
    }
    // Build the injected attach bootstrap with a static CRT so it needs no VC
    // runtime present in the target host, and embed it. Only meaningful on
    // Windows; elsewhere embed an empty placeholder the runtime never injects.
    let bootstrap = out.join("flint-bootstrap.dll");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let bootstrap_target = out.join("bootstrap-target");
        let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
            .current_dir(&root)
            .env("RUSTFLAGS", "-C target-feature=+crt-static")
            .args([
                "build",
                "--locked",
                "--release",
                "-p",
                "flint-bridge-bootstrap",
                "--target",
            ])
            .arg(&target)
            .arg("--target-dir")
            .arg(&bootstrap_target)
            .status()
            .expect("Cannot build the attach bootstrap");
        assert!(status.success(), "Attach bootstrap build failed");
        let built = bootstrap_target
            .join(&target)
            .join("release")
            .join("flint_bridge_bootstrap.dll");
        std::fs::copy(&built, &bootstrap).expect("Cannot stage the attach bootstrap");
    } else {
        std::fs::write(&bootstrap, []).expect("Cannot stage the attach bootstrap placeholder");
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
        "Flint desktop UI build failed; see the npm error above. Use cargo xtask build to prepare locked dependencies"
    );
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("Tauri build failed");
}
