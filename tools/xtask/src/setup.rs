//! Repository dependencies and Git hook preparation.
use crate::{execute, target_dir};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::Command,
};

/// The development groups own interpreter requirements; all groups share one environment.
pub(super) fn python_request(root: &Path) -> Result<String> {
    if let Ok(python) = std::env::var("UV_PYTHON") {
        return Ok(python);
    }
    let manifest: toml::Table = fs::read_to_string(root.join("bridges/pyproject.toml"))?.parse()?;
    let groups = manifest["tool"]["uv"]["dependency-groups"]
        .as_table()
        .context("Python development groups must declare interpreter requirements")?;
    let requirements = groups
        .values()
        .map(|group| {
            group["requires-python"]
                .as_str()
                .context("Python development group requires requires-python")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    Ok(requirements.into_iter().collect::<Vec<_>>().join(","))
}

fn tool_version(name: &str) -> Result<String> {
    let manifest: toml::Table = include_str!("../../../Cargo.toml").parse()?;
    Ok(manifest["workspace"]["metadata"]["tools"][name]["version"]
        .as_str()
        .context("workspace must declare the tool version")?
        .to_owned())
}

fn tools_dir(root: &Path) -> PathBuf {
    let path = target_dir(root).join("tools");
    #[cfg(windows)]
    let path = PathBuf::from(path.to_string_lossy().trim_start_matches(r"\\?\"));
    path
}

fn prek_path(root: &Path) -> PathBuf {
    tools_dir(root)
        .join("bin")
        .join(if cfg!(windows) { "prek.exe" } else { "prek" })
}

fn install_prek(root: &Path) -> Result<PathBuf> {
    let version = tool_version("prek")?;
    let binary = prek_path(root);
    let expected = format!("prek {version}");
    let ready = Command::new(&binary)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == expected);
    if !ready {
        let directory = tools_dir(root);
        let build = directory.join("build");
        execute(
            root,
            "cargo",
            &[
                "install",
                "--locked",
                "--force",
                "--version",
                &version,
                "--root",
                directory.to_str().context("tool directory is not UTF-8")?,
                "--target-dir",
                build.to_str().context("tool build directory is not UTF-8")?,
                "prek",
            ],
            &[],
        )?;
    }
    Ok(binary)
}

pub(super) fn protoc_path(root: &Path) -> PathBuf {
    tools_dir(root)
        .join("protoc/bin")
        .join(if cfg!(windows) { "protoc.exe" } else { "protoc" })
}

fn install_protoc(root: &Path) -> Result<()> {
    let version = tool_version("protoc")?;
    let binary = protoc_path(root);
    let expected = format!("libprotoc {version}");
    if Command::new(&binary)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == expected)
    {
        return Ok(());
    }
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64" | "aarch64") => "win64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch_64",
        ("macos", "x86_64" | "aarch64") => "osx-universal_binary",
        (os, arch) => anyhow::bail!("no protoc distribution for {os}/{arch}"),
    };
    let manifest: toml::Table = include_str!("../../../Cargo.toml").parse()?;
    let checksum = manifest["workspace"]["metadata"]["tools"]["protoc"]["sha256"][platform]
        .as_str()
        .context("workspace must declare the protoc archive checksum")?;
    let filename = format!("protoc-{version}-{platform}.zip");
    let archive = tools_dir(root).join(&filename);
    fs::create_dir_all(tools_dir(root))?;
    let valid_archive = fs::read(&archive).is_ok_and(|bytes| format!("{:x}", Sha256::digest(bytes)) == checksum);
    if !valid_archive {
        let url = format!("https://github.com/protocolbuffers/protobuf/releases/download/v{version}/{filename}");
        let curl = if cfg!(windows) { "curl.exe" } else { "curl" };
        execute(
            root,
            curl,
            &[
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
                archive.to_str().context("protoc archive path is not UTF-8")?,
                &url,
            ],
            &[],
        )?;
    }
    let bytes = fs::read(&archive)?;
    anyhow::ensure!(
        format!("{:x}", Sha256::digest(&bytes)) == checksum,
        "protoc archive checksum mismatch"
    );
    zip::ZipArchive::new(Cursor::new(bytes))?.extract(tools_dir(root).join("protoc"))?;
    Ok(())
}

pub(super) fn run(root: &Path) -> Result<()> {
    execute(root, "cargo", &["fetch", "--locked"], &[])?;
    execute(
        root,
        "git",
        &["submodule", "update", "--init", "--", "apps/flint/cairn"],
        &[],
    )?;
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    execute(
        &root.join("apps/flint"),
        npm,
        &["ci", "--include=dev", "--engine-strict", "--no-audit", "--no-fund"],
        &[],
    )?;
    let python = python_request(root)?;
    execute(
        root,
        "uv",
        &[
            "sync",
            "--project",
            "bridges",
            "--locked",
            "--all-groups",
            "--python",
            &python,
        ],
        &[],
    )?;
    execute(
        &root.join("bridges"),
        "dotnet",
        &["restore", "Flint.slnx", "--locked-mode"],
        &[],
    )?;
    install_protoc(root)?;
    let prek = install_prek(root)?;
    execute(
        root,
        prek.to_str().context("prek path is not UTF-8")?,
        &["install", "--overwrite"],
        &[],
    )?;
    println!("Repository dependencies and Git hooks are ready");
    Ok(())
}

pub(super) fn run_hooks(root: &Path) -> Result<()> {
    let binary = prek_path(root);
    anyhow::ensure!(
        binary.is_file(),
        "Git hook runner is not prepared; run cargo xtask setup"
    );
    execute(
        root,
        binary.to_str().context("prek path is not UTF-8")?,
        &["run", "--all-files"],
        &[],
    )
}
