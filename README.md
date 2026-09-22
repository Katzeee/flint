# flint

flint is a local application execution bridge in the same stone-themed product line as lode. One Rust executable provides the CLI, shared backend, system tray, and Tauri management window. Maya and 3ds Max connect through an independently usable Python Bridge and a language-neutral Protobuf contract.

The current host integration supports active connections from Maya and 3ds Max. A plain Python host is available for scripts and testing. Process discovery reports candidate applications separately from connected bridges. Injection, Unity, and other managed runtimes are not exposed as product capabilities.

## Run flint

On Windows, the desktop interface requires WebView2, and the MSVC build uses the Microsoft Visual C++ x64 runtime. The backend and CLI do not require a Python installation. Opening flint without arguments starts or reuses the backend and opens its window. Closing the window hides it; the backend and tray remain in the same process.

```text
flint
flint status --json
flint instances --json
flint hosts --json
flint restart --json
flint stop --json
```

Business commands ensure the local backend is available. Concurrent callers share the same process through lifecycle and running locks. Stop and restart refuse while execution responses are pending, and never terminate the host application. Help, version, process discovery, and bridge export do not start a backend. CLI requests are not replayed after transport failures.

Options follow the command. The control endpoint defaults to 127.0.0.1:6322 and bridge registration to 127.0.0.1:6321. Use --host, --port, --registry-host, and --registry-port to select an endpoint. --timeout bounds each lifecycle operation; restart shares that deadline across stop and start. --no-tray runs the backend without a desktop event loop for automated tests or unattended environments. A remote backend must already be running.

FLINT_STATE_DIR selects the state directory; otherwise flint uses the platform's local application-data directory. Backend logs are under runtime/<control-port>/backend.log and workflows under workflows/. Each backend managing an endpoint must use the same state directory. Backend startup preserves completed records and marks interrupted work as failed with an explicitly unknown host outcome.

## Connect Maya or 3ds Max

Export the complete Python Bridge from the executable. This ZIP includes the bridge, generated Python protocol bindings, and the pure Python Protobuf runtime; it does not depend on the development checkout.

```text
flint bridge export --output C:/tools/flint-bridge.zip
flint start
```

Run the following from Maya's Python script editor on its main thread:

```python
import sys
sys.path.insert(0, "C:/tools/flint-bridge.zip")

import flint_bridge
bridge = flint_bridge.connect(host="maya", name="My Maya")
```

In 3ds Max's Python execution environment, use the same code with host="max". Both adapters select the application's Qt UI thread. The validated installations are Maya 2024 with CPython 3.10.8 and 3ds Max 2024.2.13 with CPython 3.10.14. Other host versions require their own integration verification.

connect also accepts address, port, and heartbeat_interval. It returns the matching bridge on repeated calls. Changing the endpoint requires an explicit disconnect, rather than silently replacing a user's integration. bridge.connected reports both channels ready, and bridge.wait_until_connected(timeout=10) waits for registration when needed. Call flint_bridge.disconnect() to stop transport and cancel queued work; its false return indicates that shutdown has not completed. Already running host code is not forcibly interrupted.

The Python subproject is self-contained under bridges/python: packages/bridge owns the host implementation and packages/protocol owns its generated messages and framing. Its uv workspace manages both packages. The ZIP is the complete distribution for hosts that do not manage pip dependencies.

## Execute and inspect

```text
flint instances --type maya --json
flint workflow --name "Inspect scene" --json
flint exec --instance-id <instance-id> --workflow-id <workflow-id> --code "print('hello')" --json
flint exec --instance-id <instance-id> --workflow-id <workflow-id> --file inspect_scene.py --json
flint execution --workflow-id <workflow-id> --execution-id 0001 --view full --json
```

exec accepts exactly one of --code, --file, or --stdin. Each bridge executes one request at a time in its own persistent namespace and uses its host adapter for thread dispatch. Source filenames are retained for tracebacks. stdout and stderr are sent incrementally and persisted in the workflow.

The backend waits up to five seconds before returning a running execution. The host keeps executing after the CLI exits. Use execution to inspect progress and completion; --view full includes source code. Exit code 0 includes accepted running work, 1 indicates an operation or execution failure, 2 indicates invalid arguments, and 130 indicates interrupted CLI waiting. A timeout or lost connection does not establish that host code has stopped.

## Build on Windows

Install Rust through rustup, Visual Studio C++ build tools with a Windows SDK, and uv 0.12.17 or newer on PATH. Then build directly:

```powershell
cargo build --locked --release
```

The executable is target/release/flint.exe. Cargo invokes uv against bridges/python to prepare Python 3.13 and the Bridge runtime dependencies from that subproject's uv.lock in an isolated environment under OUT_DIR. bridges/python/tools/package_bridge.py packages its sources, generated protocol bindings, pure Python Protobuf code, and license into the embedded Bridge ZIP. uv owns dependency downloads and caching; the repository contains no wheels or developer-local build inputs. The build does not modify development virtual environments.

Cargo.lock pins the Rust workspace. bridges/python/uv.lock pins the Python subproject; bridges/dotnet/global.json and the C# projects' NuGet lockfiles constrain .NET tooling and dependencies. Commit lockfiles and generated protocol sources. Ordinary application builds do not run protoc or require Node.js or the .NET SDK. The application's WebView2 and Visual C++ runtime requirements apply when running it, not when managing Python packages.

The standalone Bridge can be exported without another build wrapper:

```powershell
target/release/flint.exe bridge export --output flint-bridge.zip
```

The Cargo workspace separates apps/flint/src-tauri (CLI and desktop), crates/flint-core (backend and workflows), crates/flint-control-client (control requests and lifecycle), crates/flint-connect (host discovery), and crates/flint-protocol (messages and framing). The frontend is in apps/flint/src. Python and .NET each own their package configuration and component code under bridges/. The .NET subtree currently contains the protocol library and interoperability peer, not a managed host bridge. Language-neutral schemas remain in protocol/ and their generator remains a Rust tool.

## Development checks

Product integration tests are the Rust workspace member tests/integration. They drive the executable and exported Bridge ZIP, while Python fixture scripts perform only host-side setup and operations. Build the executable before running them. FLINT_TEST_PYTHON selects the interpreter used as a disposable plain-Python host; an actual python executable on PATH is used when it is unset.

```powershell
cargo build --locked
$env:FLINT_TEST_PYTHON = uv python find --system 3.13
cargo test --workspace --locked
uv run --directory bridges/python --locked --package flint-bridge --group test --python 3.13 pytest -q
```

FLINT_BINARY selects a different application build. Python component tests live under bridges/python/tests and include execution strategies and portable packaging. No root Python workspace is required. The Rust integration package's Python 3.7, C#, and DCC checks are ignored by default because they require additional installations. Tests never install toolchains themselves.

Set FLINT_PYTHON37 to an existing Python 3.7 interpreter, then run the compatibility check explicitly. C# interoperability requires the SDK selected by bridges/dotnet/global.json; the Rust driver builds from that directory so SDK selection remains local to the .NET subproject. FLINT_DOTNET can select a dotnet executable outside PATH.

```powershell
cargo test --locked -p flint-integration-tests --test application python37 -- --ignored
cargo test --locked -p flint-integration-tests --test protocol_interop csharp -- --ignored
```

Maya/Max tests create only fresh empty host processes and preserve evidence under target/integration-artifacts/. For example:

```powershell
$env:FLINT_MAYA_EXE = 'C:/Program Files/Autodesk/Maya2024/bin/maya.exe'
cargo test --locked -p flint-integration-tests --test hosts maya -- --ignored --nocapture
```

For Max, set FLINT_MAX_EXE to its 3dsmax.exe and replace the test filter maya with max. Protocol contracts live in protocol/flint_protocol/v1; their field comments describe connection and execution semantics. Generated messages live in the Rust protocol crate, the Python protocol package, and the .NET protocol project. cargo codegen regenerates them and cargo codegen --check verifies them, using protoc 24.4 on PATH or through PROTOC. Commit schema changes and regenerated bindings together. CI checks the application build, Rust integration tests, scoped Python component tests, and generated-code consistency.
