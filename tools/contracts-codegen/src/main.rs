use anyhow::{Context, Result};
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

fn files_under(root: &Path, directory: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut files = vec![];
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(files_under(root, &path)?);
        } else {
            files.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn managed(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    (name.ends_with(".rs") && name != "mod.rs") || name.ends_with(".ts")
}

fn synchronize(generated: &Path, destination: &Path, check: bool) -> Result<()> {
    let emitted = files_under(generated, generated)?;
    for existing in files_under(destination, destination)? {
        if managed(&existing) && !emitted.contains(&existing) {
            let path = destination.join(existing);
            if check {
                return Err(anyhow::anyhow!(
                    "Obsolete generated file: {}",
                    path.display()
                ));
            }
            fs::remove_file(path)?;
        }
    }
    for relative in emitted {
        let target = destination.join(&relative);
        let content = fs::read_to_string(generated.join(relative))?;
        let content = format!("{}\n", content.trim_end());
        if check {
            let current = fs::read_to_string(&target)
                .with_context(|| format!("cannot read generated file {}", target.display()))?;
            if current.replace("\r\n", "\n") != content.replace("\r\n", "\n") {
                return Err(anyhow::anyhow!(
                    "Stale generated file: {}",
                    target.display()
                ));
            }
        } else {
            fs::create_dir_all(target.parent().unwrap())?;
            fs::write(target, content)?;
        }
    }
    println!(
        "{} {}",
        if check { "Checked" } else { "Generated" },
        destination.display()
    );
    Ok(())
}

fn generate_ipc(root: &Path, temporary: &Path, check: bool) -> Result<()> {
    let generated = temporary.join("typescript");
    fs::create_dir_all(&generated)?;
    // Compile after protocol generation so IPC sees the current Rust contract.
    // The library-only build does not consume the frontend it is generating.
    let status = Command::new("cargo")
        .current_dir(root)
        .args([
            "run",
            "--locked",
            "-p",
            "flint",
            "--no-default-features",
            "--features",
            "bindings",
            "--bin",
            "flint-bindings",
            "--",
        ])
        .arg(generated.join("bindings.ts"))
        .status()
        .context("cannot run the desktop binding exporter")?;
    anyhow::ensure!(status.success(), "desktop binding export failed");
    synchronize(&generated, &root.join("apps/flint/src/generated"), check)
}

fn generate_protocol(root: &Path, temporary: &Path, check: bool) -> Result<()> {
    let protocol = root.join("protocol");
    let protoc = std::env::var_os("PROTOC")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .unwrap_or_else(|| PathBuf::from("protoc"));
    let version = Command::new(&protoc)
        .arg("--version")
        .output()
        .with_context(|| {
            format!(
                "cannot run {}; provide protoc 24.4 on PATH or through PROTOC",
                protoc.display()
            )
        })?;
    if !version.status.success()
        || String::from_utf8_lossy(&version.stdout).trim() != "libprotoc 24.4"
    {
        anyhow::bail!("protocol generation requires protoc 24.4");
    }
    let generated = temporary.join("rust");
    fs::create_dir_all(&generated)?;
    std::env::set_var("PROTOC", &protoc);
    prost_build::Config::new()
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .type_attribute(
            ".",
            "#[cfg_attr(feature = \"typescript\", derive(specta::Type))]",
        )
        .enum_attribute("ExecutionStatus", "#[serde(rename_all = \"snake_case\")]")
        .field_attribute(
            "ExecutionResult.status",
            "#[cfg_attr(feature = \"typescript\", specta(type = ExecutionStatus))]",
        )
        .field_attribute(
            "ExecutionResult.status",
            "#[serde(with = \"serde_with::As::<serde_with::TryFromInto<ExecutionStatus>>\")]",
        )
        .field_attribute(
            "GetExecutionResponse.status",
            "#[cfg_attr(feature = \"typescript\", specta(type = ExecutionStatus))]",
        )
        .field_attribute(
            "GetExecutionResponse.status",
            "#[serde(with = \"serde_with::As::<serde_with::TryFromInto<ExecutionStatus>>\")]",
        )
        .field_attribute(
            "ExecutionResult.traceback",
            "#[serde(skip_serializing_if = \"Option::is_none\")]",
        )
        .field_attribute(
            "ExecutionResult.error",
            "#[serde(skip_serializing_if = \"Option::is_none\")]",
        )
        .field_attribute(
            "GetExecutionResponse.code",
            "#[serde(skip_serializing_if = \"Option::is_none\")]",
        )
        .out_dir(&generated)
        .compile_protos(
            &[protocol.join("flint_protocol/v1/envelope.proto")],
            &[protocol],
        )?;
    synchronize(
        &generated,
        &root.join("crates/flint-contracts/src/protocol/generated"),
        check,
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("cargo codegen [--check]");
        return Ok(());
    }
    if !(args.is_empty() || args == ["--check"]) {
        return Err(anyhow::anyhow!(
            "expected no arguments (generate), or --check"
        ));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    // protoc compares paths textually and does not normalize Windows verbatim prefixes.
    #[cfg(windows)]
    let root = PathBuf::from(root.to_string_lossy().trim_start_matches(r"\\?\"));
    let temporary = tempfile::Builder::new()
        .prefix("flint-contracts-")
        .tempdir()?;
    let check = !args.is_empty();
    generate_protocol(&root, temporary.path(), check)?;
    generate_ipc(&root, temporary.path(), check)
}
