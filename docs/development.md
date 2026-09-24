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

The resulting executable is target/release/flint.exe and includes the Bridge export packages for Python, C#, and Unity. The build compiles the native connection core from the locked Rust workspace and places it in each package. Confirm that the executable starts and exposes its command interface:

```powershell
.\target\release\flint.exe --version
.\target\release\flint.exe --help
```

Setup is complete when the build succeeds and both commands exit successfully. Run the executable without arguments to open its desktop window, then follow the [usage guide](../README.md) to connect a host and execute code.

## Test

Run `cargo test --workspace --locked` from the repository root. Cargo builds the application executable used by the product integration tests, so a separate `cargo build` is not required. The test driver uses uv to locate Python 3.13 unless `FLINT_TEST_PYTHON` selects an interpreter explicitly.

The Unity product integration test uses a fresh temporary project and a Unity 2022.3 Mono Editor. Set `FLINT_UNITY_EXE` to that Editor's `Unity.exe`, then run `cargo test -p flint --test hosts unity_active_connection --locked -- --ignored --nocapture`. The test installs the exported Unity package, checks connection, C# execution and output, exception reporting, and reconnection, then stops only the Editor it started.
