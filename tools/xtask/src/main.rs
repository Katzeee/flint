use std::{
    env,
    error::Error,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

struct Suite {
    name: &'static str,
    default: bool,
    run: fn(&Path) -> Result<()>,
}

const SUITES: &[Suite] = &[
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

fn rust(root: &Path) -> Result<()> {
    execute(root, "cargo", &["test", "--workspace", "--locked"], &[])
}

fn gui(root: &Path) -> Result<()> {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let app = root.join("apps/flint");
    execute(&app, npm, &["run", "typecheck"], &[])?;
    execute(&app, npm, &["test"], &[])
}

fn python(root: &Path) -> Result<()> {
    execute(
        root,
        "uv",
        &[
            "run",
            "--directory",
            "bridges/python",
            "--locked",
            "--package",
            "flint-bridge",
            "--group",
            "test",
            "--python",
            ">=3.11,<3.15",
            "pytest",
            "-q",
        ],
        &[],
    )?;
    execute(
        root,
        "cargo",
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
        &[],
    )
}

fn csharp(root: &Path) -> Result<()> {
    execute(
        root,
        "cargo",
        &["build", "--locked", "--package", "flint-bridge-core"],
        &[],
    )?;
    let core = target_dir(root).join("debug/flint_bridge_core.dll");
    execute(
        &root.join("bridges/dotnet"),
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
    execute(
        root,
        "cargo",
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
        &[],
    )
}

fn hosts(root: &Path) -> Result<()> {
    execute(
        root,
        "cargo",
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
        &[],
    )
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
                format!("`{program}` is not on PATH; see docs/development.md").into()
            }
            _ => Box::<dyn Error>::from(error),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}").into())
    }
}

fn usage() -> String {
    let names: Vec<_> = SUITES.iter().map(|suite| suite.name).collect();
    format!(
        "Usage: cargo xtask test [{}]...\nWithout suite names, runs every default suite.",
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
                    .ok_or_else(|| format!("Unknown suite `{name}`\n{}", usage()))
            })
            .collect::<std::result::Result<_, _>>()?
    };
    for suite in selected {
        println!("[{}]", suite.name);
        (suite.run)(root).map_err(|error| format!("[{}] {error}", suite.name))?;
    }
    Ok(())
}

fn dispatch(args: &[String]) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?;
    match args.split_first() {
        Some((command, names)) if command == "test" => test(&root, names),
        _ => Err(usage().into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match dispatch(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
