use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

fn files_under(root: &Path, directory: &Path, found: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<_> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_under(root, &path, found);
        } else {
            let relative = path.strip_prefix(root).unwrap().to_string_lossy();
            found.push((relative.replace('\\', "/"), path));
        }
    }
}

fn tree(root: &Path) -> Vec<(String, PathBuf)> {
    println!("cargo:rerun-if-changed={}", root.display());
    let mut found = vec![];
    files_under(root, root, &mut found);
    found
}

fn sources(directory: &Path, extension: &str, prefix: &str) -> Vec<(String, PathBuf)> {
    println!("cargo:rerun-if-changed={}", directory.display());
    let mut found: Vec<_> = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", directory.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|value| value == extension))
        .map(|path| {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            (format!("{prefix}{name}"), path)
        })
        .collect();
    found.sort();
    found
}

/// The Python library exactly as `bridges/pyproject.toml` declares its packages.
fn python_library(bridges: &Path) -> Vec<(String, PathBuf)> {
    let manifest = bridges.join("pyproject.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let manifest: toml::Table = fs::read_to_string(&manifest).unwrap().parse().unwrap();
    let setuptools = &manifest["tool"]["setuptools"];
    let directories = setuptools["package-dir"].as_table().unwrap();
    setuptools["packages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|package| {
            let package = package.as_str().unwrap();
            let directory = directories[package].as_str().unwrap();
            sources(
                &bridges.join(directory),
                "py",
                &format!("{}/", package.replace('.', "/")),
            )
        })
        .collect()
}

fn table(output: &mut String, name: &str, files: &[(String, PathBuf)]) {
    writeln!(output, "pub(crate) static {name}: &[(&str, &[u8])] = &[").unwrap();
    for (path, source) in files {
        writeln!(
            output,
            "    ({path:?}, include_bytes!({:?})),",
            source.display().to_string()
        )
        .unwrap();
    }
    writeln!(output, "];").unwrap();
}

/// The Unity adapter assembly compiled from the Unity host and the .NET binding.
fn unity_adapter(bridges: &Path, out: &Path) -> PathBuf {
    let project = bridges.join("hosts/unity");
    for file in fs::read_dir(&project).unwrap().map(|entry| entry.unwrap().path()) {
        if file.is_file() {
            println!("cargo:rerun-if-changed={}", file.display());
        }
    }
    println!("cargo:rerun-if-changed={}", bridges.join("global.json").display());
    let build = out.join("unity-adapter");
    let status = std::process::Command::new("dotnet")
        .current_dir(&project)
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
        .arg(&build)
        .status()
        .expect("dotnet (from bridges/global.json) is required to build the Unity adapter");
    assert!(status.success(), "Unity adapter build failed");
    build.join("Flint.Unity.dll")
}

/// Build a workspace cdylib for the current target in its own target directory,
/// since the outer build holds the workspace's.
fn workspace_library(root: &Path, out: &Path, package: &str, rustflags: &str) -> PathBuf {
    for input in ["Cargo.toml", "Cargo.lock"] {
        println!("cargo:rerun-if-changed={}", root.join(input).display());
    }
    for directory in ["flint-contracts", "flint-bridge-core", "flint-bridge-bootstrap"] {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("crates").join(directory).display()
        );
    }
    let target = std::env::var("TARGET").unwrap();
    let target_dir = out.join(package);
    let status = std::process::Command::new(std::env::var_os("CARGO").unwrap())
        .current_dir(root)
        // Cargo gives build scripts CARGO_ENCODED_RUSTFLAGS, which overrides RUSTFLAGS.
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env("RUSTFLAGS", rustflags)
        .args(["build", "--locked", "--release", "-p", package, "--target", &target])
        .arg("--target-dir")
        .arg(&target_dir)
        .status()
        .unwrap_or_else(|error| panic!("cannot build {package}: {error}"));
    assert!(status.success(), "{package} build failed");
    let name = package.replace('-', "_");
    let file = match std::env::var("CARGO_CFG_TARGET_OS").unwrap().as_str() {
        "windows" => format!("{name}.dll"),
        "macos" => format!("lib{name}.dylib"),
        _ => format!("lib{name}.so"),
    };
    target_dir.join(&target).join("release").join(file)
}

fn binary(output: &mut String, name: &str, path: &Path) {
    writeln!(
        output,
        "pub(crate) static {name}: &[u8] = include_bytes!({:?});",
        path.display().to_string()
    )
    .unwrap();
}

fn main() {
    let bridges = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../bridges")
        .canonicalize()
        .unwrap();
    // include_bytes! reads ordinary paths; keep Windows verbatim prefixes out of them.
    let bridges = PathBuf::from(bridges.to_string_lossy().trim_start_matches(r"\\?\"));
    let mut output = String::new();
    table(&mut output, "PYTHON_LIBRARY", &python_library(&bridges));
    table(
        &mut output,
        "DOTNET_BINDING",
        &sources(&bridges.join("platforms/dotnet/src/Flint.Bridge"), "cs", ""),
    );
    table(&mut output, "UNITY_PACKAGE", &tree(&bridges.join("hosts/unity/upm")));
    let root = bridges.parent().unwrap();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    binary(
        &mut output,
        "NATIVE_CORE",
        &workspace_library(root, &out, "flint-bridge-core", ""),
    );
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    if os == "windows" {
        // The bootstrap links the C runtime statically so it needs none in the target host.
        let bootstrap = workspace_library(root, &out, "flint-bridge-bootstrap", "-C target-feature=+crt-static");
        binary(&mut output, "BOOTSTRAP", &bootstrap);
    }
    if os == "windows" && arch == "x86_64" {
        binary(&mut output, "UNITY_ADAPTER", &unity_adapter(&bridges, &out));
    }
    fs::write(out.join("embedded.rs"), output).unwrap();
}
