# Windows development environment

Prepare the checkout through xtask, then build and verify the application. Follow the [development guide](contributing.md) when changing the repository.

## Prerequisites

Install Git, Rust through rustup, Node.js with npm, the .NET SDK, uv, and Python. Use the versions declared by [rust-toolchain.toml](../rust-toolchain.toml), the frontend's [engines](../apps/flint/package.json), [global.json](../bridges/global.json), and the [Python tooling](../bridges/pyproject.toml).

Windows builds also require Visual Studio Build Tools with the Desktop development with C++ workload and a Windows SDK. Desktop runtime prerequisites are listed under [Run flint](../README.md#run-flint).

## Initialize the checkout

From the repository root:

```powershell
cargo xtask setup
```

Setup prepares the repository's development dependencies and registers its Git hook. Preparation steps belong to [xtask](../tools/xtask/src/setup.rs); tool versions are declared in the [Cargo workspace](../Cargo.toml).

Initialization requires network access on a fresh checkout. Rerun setup after dependency changes or removing prepared dependencies. Existing Python interpreters are selected from the environment; set `UV_PYTHON` to choose one explicitly.

## Build and verify

After setup, build directly with Cargo:

```powershell
cargo build --locked
.\target\debug\flint.exe --version
.\target\debug\flint.exe --help
```

Verify that both commands exit successfully, then open the executable without arguments and confirm the desktop opens. Continue with the [usage guide](../README.md) to connect a host.

Use `cargo check` for compilation checks or `cargo build --locked --release` for an optimized executable. For repository checks and tests, follow the [development guide](contributing.md#formatting-and-lints).
