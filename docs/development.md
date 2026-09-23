# Development environment and first build

This guide takes a Windows source checkout to a working flint executable. Project development conventions are in [CLAUDE.md](../CLAUDE.md).

## Prepare the environment

Install Rust through rustup, Visual Studio Build Tools with the Desktop development with C++ workload and a Windows SDK, and uv. Make cargo and uv available on PATH. The repository selects its Rust toolchain through [rust-toolchain.toml](../rust-toolchain.toml); the Python subproject declares its uv requirements in [pyproject.toml](../bridges/python/pyproject.toml).

The first build needs network access to obtain toolchains and dependencies. Cargo uses uv to run the dependency-free Bridge packager with Python 3.13; no project environment or manually prepared Python installation is needed for the application build. To open the desktop window, install the runtime dependencies listed under [Run flint](../README.md#run-flint).

## Build and run

From the repository root in PowerShell:

```powershell
cargo build --locked --release
```

The resulting executable is target/release/flint.exe and includes the Python Bridge and its native connection core. The build compiles that core from the locked Rust workspace and places it inside the exported Bridge ZIP. Confirm that the executable starts and exposes its command interface:

```powershell
.\target\release\flint.exe --version
.\target\release\flint.exe --help
```

Setup is complete when the build succeeds and both commands exit successfully. Run the executable without arguments to open its desktop window, then follow the [usage guide](../README.md) to connect a host and execute code.

## Test

Run `cargo test --workspace --locked` from the repository root. Cargo builds the application executable used by the product integration tests, so a separate `cargo build` is not required. The test driver uses uv to locate Python 3.13 unless `FLINT_TEST_PYTHON` selects an interpreter explicitly.
