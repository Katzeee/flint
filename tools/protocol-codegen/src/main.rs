//! Generate the Rust protocol binding without depending on the application or Node.js.
use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;

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
    name.ends_with(".rs") && name != "mod.rs"
}

fn synchronize(generated: &Path, destination: &Path, check: bool) -> Result<()> {
    let emitted = files_under(generated, generated)?;
    for existing in files_under(destination, destination)? {
        if managed(&existing) && !emitted.contains(&existing) {
            let path = destination.join(existing);
            if check {
                return Err(format!("Obsolete generated file: {}", path.display()).into());
            }
            fs::remove_file(path)?;
        }
    }
    for relative in emitted {
        let target = destination.join(&relative);
        let content = fs::read_to_string(generated.join(relative))?;
        if check {
            let current = fs::read_to_string(&target)
                .map_err(|e| format!("Cannot read generated file {}: {e}", target.display()))?;
            if current.replace("\r\n", "\n") != content.replace("\r\n", "\n") {
                return Err(format!("Stale generated file: {}", target.display()).into());
            }
        } else {
            fs::create_dir_all(target.parent().unwrap())?;
            fs::write(target, content)?;
        }
    }
    println!("{} Rust", if check { "Checked" } else { "Generated" });
    Ok(())
}

fn generate(root: &Path, temporary: &Path, check: bool) -> Result<()> {
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
        .map_err(|e| {
            format!(
                "Cannot run {}: {e}. Provide protoc 24.4 on PATH or through PROTOC.",
                protoc.display()
            )
        })?;
    if !version.status.success()
        || String::from_utf8_lossy(&version.stdout).trim() != "libprotoc 24.4"
    {
        return Err("Protocol generation requires protoc 24.4".into());
    }
    let generated = temporary.join("rust");
    fs::create_dir_all(&generated)?;
    std::env::set_var("PROTOC", &protoc);
    prost_build::Config::new()
        .out_dir(&generated)
        .compile_protos(
            &[protocol.join("flint_protocol/v1/envelope.proto")],
            &[protocol],
        )?;
    synchronize(
        &generated,
        &root.join("crates/flint-protocol/src/generated"),
        check,
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("cargo run --locked -p protocol-codegen -- [--check]");
        return Ok(());
    }
    if !(args.is_empty() || args == ["--check"]) {
        return Err("Expected no arguments (generate), or --check".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    // protoc compares paths textually and does not normalize Windows verbatim prefixes.
    #[cfg(windows)]
    let root = PathBuf::from(root.to_string_lossy().trim_start_matches(r"\\?\"));
    let temporary = tempfile::Builder::new()
        .prefix("flint-protocol-")
        .tempdir()?;
    generate(&root, temporary.path(), !args.is_empty())
}
