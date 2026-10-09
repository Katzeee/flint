use std::{
    env,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use anyhow::{Context, Result};

struct Suite {
    name: &'static str,
    default: bool,
    run: fn(&Path) -> Result<()>,
}

const SUITES: &[Suite] = &[
    Suite {
        name: "lint",
        default: true,
        run: lint,
    },
    Suite {
        name: "rust",
        default: true,
        run: rust,
    },
    Suite {
        name: "gui",
        default: true,
        run: gui,
    },
    Suite {
        name: "python",
        default: true,
        run: python,
    },
    Suite {
        name: "csharp",
        default: true,
        run: csharp,
    },
    Suite {
        name: "hosts",
        default: false,
        run: hosts,
    },
];

fn prepare(root: &Path) -> Result<()> {
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
        &["ci", "--include=dev", "--no-audit", "--no-fund"],
        &[],
    )
}

fn build(root: &Path, args: &[String]) -> Result<()> {
    let release = match args {
        [] => false,
        [flag] if flag == "--release" => true,
        _ => return Err(anyhow::anyhow!(usage())),
    };
    prepare(root)?;
    let mut args = vec!["build", "--locked", "--package", "flint"];
    if release {
        args.push("--release");
    }
    execute(root, "cargo", &args, &[])
}

fn lint(root: &Path) -> Result<()> {
    execute(root, "cargo", &["fmt", "--all", "--", "--check"], &[])?;
    let target = target_dir(root).join("lint");
    #[cfg(windows)]
    let target = PathBuf::from(target.to_string_lossy().trim_start_matches(r"\\?\"));
    execute(
        root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        &[("CARGO_TARGET_DIR", target.as_os_str())],
    )
}

fn rust(root: &Path) -> Result<()> {
    cargo_test(root, &["test", "--workspace", "--locked"])
}

fn gui_checks(root: &Path) -> Result<()> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let app = root.join("apps/flint");
    execute(&app, npm, &["run", "format:check"], &[])?;
    execute(&app, npm, &["run", "lint"], &[])
}

fn gui(root: &Path) -> Result<()> {
    gui_checks(root)?;
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let app = root.join("apps/flint");
    execute(&app, npm, &["run", "typecheck"], &[])?;
    execute(&app, npm, &["test"], &[])
}

fn python_tool(root: &Path, command: &[&str]) -> Result<()> {
    let mut args = vec![
        "run",
        "--project",
        "bridges",
        "--locked",
        "--group",
        "lint",
        "--python",
        ">=3.11,<3.15",
    ];
    args.extend_from_slice(command);
    execute(root, "uv", &args, &[])
}

fn python_checks(root: &Path) -> Result<()> {
    python_tool(
        root,
        &["ruff", "format", "--check", "--config", "bridges/pyproject.toml", "."],
    )?;
    python_tool(root, &["ruff", "check", "--config", "bridges/pyproject.toml", "."])
}

fn csharp_checks(root: &Path) -> Result<()> {
    let bridges = root.join("bridges");
    execute(&bridges, "dotnet", &["restore", "Flint.slnx", "--locked-mode"], &[])?;
    execute(
        &bridges,
        "dotnet",
        &["format", "Flint.slnx", "--verify-no-changes", "--no-restore"],
        &[],
    )?;
    for folder in ["hosts/unity/upm", "../tests/fixtures"] {
        execute(
            &bridges,
            "dotnet",
            &["format", "whitespace", folder, "--folder", "--verify-no-changes"],
            &[],
        )?;
    }
    execute(
        &bridges,
        "dotnet",
        &["build", "Flint.slnx", "--no-restore", "--warnaserror", "--nologo"],
        &[],
    )
}

fn check(root: &Path, names: &[String]) -> Result<()> {
    let selected: Vec<&str> = if names.is_empty() {
        vec!["rust", "gui", "python", "csharp"]
    } else {
        names.iter().map(String::as_str).collect()
    };
    for name in selected {
        match name {
            "rust" => lint(root),
            "gui" => gui_checks(root),
            "python" => python_checks(root),
            "csharp" => csharp_checks(root),
            _ => anyhow::bail!("unknown check {name:?}; expected rust, gui, python, or csharp"),
        }
        .with_context(|| format!("[{name}]"))?;
    }
    Ok(())
}

fn python_environment(root: &Path, command: &[&str]) -> Result<()> {
    let mut args = vec![
        "run",
        "--directory",
        "bridges",
        "--locked",
        "--package",
        "flint-bridge",
        "--group",
        "test",
        "--python",
        ">=3.11,<3.15",
    ];
    args.extend_from_slice(command);
    execute(root, "uv", &args, &[])
}

fn python(root: &Path) -> Result<()> {
    python_checks(root)?;
    python_environment(root, &["pytest", "-q"])?;
    python_environment(
        root,
        &[
            "cargo",
            "test",
            "--locked",
            "--package",
            "flint-hosts",
            "--",
            "platform::python::",
            "--ignored",
        ],
    )?;
    cargo_test(
        root,
        &[
            "test",
            "--locked",
            "--package",
            "flint",
            "--test",
            "product",
            "--",
            "runtime::python::",
            "--ignored",
        ],
    )
}

fn csharp(root: &Path) -> Result<()> {
    csharp_checks(root)?;
    execute(
        root,
        "cargo",
        &["build", "--locked", "--package", "flint-bridge-core"],
        &[],
    )?;
    let core = target_dir(root).join("debug/flint_bridge_core.dll");
    execute(
        &root.join("bridges/platforms/dotnet"),
        "dotnet",
        &[
            "test",
            "--project",
            "tests/Flint.Bridge.Tests/Flint.Bridge.Tests.csproj",
            "-p:RestoreLockedMode=true",
            "-p:NuGetAudit=false",
        ],
        &[("FLINT_BRIDGE_CORE", core.as_os_str())],
    )?;
    cargo_test(
        root,
        &[
            "test",
            "--locked",
            "--package",
            "flint",
            "--test",
            "product",
            "--",
            "runtime::csharp::",
            "--ignored",
        ],
    )
}

fn hosts(root: &Path) -> Result<()> {
    cargo_test(
        root,
        &[
            "test",
            "--locked",
            "--package",
            "flint",
            "--test",
            "product",
            "--",
            "hosts::",
            "--ignored",
            "--test-threads=1",
        ],
    )
}

fn cargo_test(root: &Path, args: &[&str]) -> Result<()> {
    let mut args = args.to_vec();
    let options = args.iter().position(|arg| *arg == "--").unwrap_or(args.len());
    args.splice(options..options, ["--features", "flint/test-runtime"]);
    let target = target_dir(root).join("tests");
    // Tauri's permission globbing requires ordinary Windows paths.
    #[cfg(windows)]
    let target = PathBuf::from(target.to_string_lossy().trim_start_matches(r"\\?\"));
    execute(root, "cargo", &args, &[("CARGO_TARGET_DIR", target.as_os_str())])
}

fn target_dir(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(|path| root.join(path))
        .unwrap_or_else(|| root.join("target"))
}

fn execute(directory: &Path, program: &str, args: &[&str], envs: &[(&str, &OsStr)]) -> Result<()> {
    println!("> {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .envs(envs.iter().copied())
        .current_dir(directory)
        .status()
        .map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => {
                anyhow::Error::new(error).context(format!("`{program}` is not on PATH; see docs/development.md"))
            }
            _ => anyhow::Error::from(error),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{program} exited with {status}"))
    }
}

fn usage() -> String {
    let names: Vec<_> = SUITES.iter().map(|suite| suite.name).collect();
    format!(
        "Usage: cargo xtask build [--release]\n       cargo xtask test [{}]...\n       cargo xtask check [rust|gui|python|csharp]...\n       cargo xtask hooks\nBuild and test prepare the submodule and locked frontend dependencies.\nWithout suite names, test runs every default suite.",
        names.join("|")
    )
}

fn test(root: &Path, names: &[String]) -> Result<()> {
    let selected: Vec<&Suite> = if names.is_empty() {
        SUITES.iter().filter(|suite| suite.default).collect()
    } else {
        names
            .iter()
            .map(|name| {
                SUITES
                    .iter()
                    .find(|suite| suite.name == name)
                    .ok_or_else(|| anyhow::anyhow!("unknown suite `{name}`\n{}", usage()))
            })
            .collect::<std::result::Result<_, _>>()?
    };
    prepare(root)?;
    for suite in selected {
        println!("[{}]", suite.name);
        (suite.run)(root).with_context(|| format!("[{}]", suite.name))?;
    }
    Ok(())
}

fn dispatch(args: &[String]) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    match args.split_first() {
        Some((command, args)) if command == "build" => build(&root, args),
        Some((command, names)) if command == "test" => test(&root, names),
        Some((command, names)) if command == "check" => check(&root, names),
        Some((command, args)) if command == "hooks" && args.is_empty() => {
            python_tool(&root, &["pre-commit", "install"])
        }
        _ => Err(anyhow::anyhow!(usage())),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}
