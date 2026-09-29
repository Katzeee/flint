# Development environment and first build

This guide takes a Windows source checkout to a working flint executable. For implementation and testing conventions, see the [development guide](contributing.md).

## Prepare the environment

Install Git, Rust through rustup, Visual Studio Build Tools with the Desktop development with C++ workload and a Windows SDK, Node.js 22.12 or later, and uv. The C# tests also require the .NET SDK selected by [global.json](../bridges/dotnet/global.json). Make git, cargo, node, npm, uv, and, when testing C#, dotnet available on PATH.

The first build needs network access to obtain toolchains and dependencies. Install Python 3.11 through 3.14 yourself; uv discovers it on PATH, in the Windows registry, or among uv-managed installations, and never downloads one for this repository. Set `UV_PYTHON` if several interpreters qualify. The exported Python Bridge runs in the host's interpreter and supports Python 3.7 or later. To open the desktop window, install the runtime dependencies listed under [Run flint](../README.md#run-flint).

## Build and run

From the repository root in PowerShell, use the complete build entry point:

```powershell
cargo xtask build
```

The command prepares the recorded Cairn submodule and locked frontend dependencies, then builds the application with its embedded desktop UI.

For local iteration after preparation, use `cargo build` or `cargo check`. Return to `cargo xtask build` after pulling dependency or submodule changes, or removing `node_modules`.

The debug executable is `target/debug/flint.exe`. For an optimized executable, use:

```powershell
cargo xtask build --release
```

The resulting executable is target/release/flint.exe and includes the Bridge export packages for Python, Blender, C#, and Unity. The build compiles the native connection core from the locked Rust workspace and places it in each package. Confirm that the executable starts and exposes its command interface:

```powershell
.\target\release\flint.exe --version
.\target\release\flint.exe --help
```

Setup is complete when the build succeeds and both commands exit successfully. Run the executable without arguments to open its desktop window, then follow the [usage guide](../README.md) to connect a host and execute code.
