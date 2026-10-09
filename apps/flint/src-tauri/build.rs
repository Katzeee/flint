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

fn build_frontend() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
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
        "apps/flint/src/generated",
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
}

fn main() {
    if std::env::var_os("CARGO_FEATURE_DESKTOP").is_some() {
        build_frontend();
    }
    println!("cargo:rerun-if-changed=windows-app-manifest.xml");
    let windows = tauri_build::WindowsAttributes::new()
        .app_manifest(include_str!("windows-app-manifest.xml"));
    let attributes = tauri_build::Attributes::new().windows_attributes(windows);
    tauri_build::try_build(attributes).expect("Tauri build failed");
}
