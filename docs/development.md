# Development environment and first build

This guide takes a Windows source checkout to a working flint executable. Project development conventions are in [CLAUDE.md](../CLAUDE.md).

## Prepare the environment

Install Rust through rustup, Visual Studio Build Tools with the Desktop development with C++ workload and a Windows SDK, and uv. Make cargo and uv available on PATH. The repository selects its Rust toolchain through [rust-toolchain.toml](../rust-toolchain.toml); the Python subproject declares its uv requirements in [pyproject.toml](../bridges/python/pyproject.toml).

The first build needs network access to obtain toolchains and dependencies. Cargo uses uv to prepare the Bridge's Python build environment automatically. No manually prepared Python environment is needed for the application build. To open the desktop window, install the runtime dependencies listed under [Run flint](../README.md#run-flint).

## Build and run

From the repository root in PowerShell:

```powershell
cargo build --locked --release
```

The resulting executable is target/release/flint.exe and includes the portable Python Bridge. Confirm that it starts and exposes its command interface:

```powershell
.\target\release\flint.exe --version
.\target\release\flint.exe --help
```

Setup is complete when the build succeeds and both commands exit successfully. Run the executable without arguments to open its desktop window, then follow the [usage guide](../README.md) to connect a host and execute code.
