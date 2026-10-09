# Development guide

Conventions for where code belongs, how errors and protocols evolve, how builds stay reproducible, and where tests go. Prepare a Windows checkout and verify the first build with the [environment guide](development.md).

## Ownership

Place a type or behavior in the component that defines its meaning, using the [glossary](../CONTEXT.md) to resolve terms; callers depend on that owner. Within a crate, modules and visibility express ownership.

A crate exists for one of three reasons, named in its row below:

- **Artifact**: it builds into a separate binary.
- **Contract**: it holds what separately built binaries exchange. `flint-contracts` is the only one.
- **Service**: a domain the executable composes, which builds and tests without Tauri.

Everything else is a module of the crate that owns its meaning, including code several crates call. A linked artifact contains only the code it reaches, so dependency weight alone justifies no crate. A new crate adds its row here.

| Crate | Reason | Owns |
|---|---|---|
| `crates/flint-contracts` | Contract | `protocol`: wire messages generated from the schemas in `protocol/`, framing, heartbeat timing, and the failure code catalog, behind the `protocol` feature. `attach`: the runtime plan and the files the injector and bootstrap exchange |
| `crates/flint-backend` | Service | The backend service: control and Bridge listeners, request dispatch, connected instances, executions, and workflow records. `config`: its deployment conventions, namely control and Bridge endpoints, state directory, and runtime lock files |
| `crates/flint-hosts` | Service | Built-in host identities (`HostKind`), process discovery, window information, previews, focus, language platforms (`Platform`) with their libraries and attach entries, each host's layouts with the Bridge files and binaries they embed and the assembler that writes them, and bootstrap injection |
| `crates/flint-bridge-core` | Artifact | Bridge core and its C ABI for host adapters, and the process claim that limits a host process to one Bridge |
| `crates/flint-bridge-bootstrap` | Artifact | The injected library: it executes a runtime plan through the host runtime and knows no host or platform |
| `apps/flint/src-tauri` | Artifact | The product API, CLI commands, the control client and local backend lifecycle, Tauri window, tray, and IPC |

Dependencies stay acyclic and follow ownership. Tauri and UI dependencies stay in `apps/flint`; the Bridge core and bootstrap depend only on `flint-contracts`, the bootstrap without its `protocol` feature.

The public product API is `application` in `apps/flint/src-tauri`. It exposes typed operations independently of terminal input and window presentation. Application construction loads the runtime from the single configuration entry in `flint-backend::config`; callers do not supply runtime configuration or per-invocation overrides. The executable and its backend child use that same entry. Local host inspection, focus, and export use `flint-hosts` without a backend connection or its configuration; backend operations use the control protocol. API queries never start a backend; startup and restart are explicit operations, and attach establishes a backend before injecting a Bridge. Each frontend starts the backend for what its user explicitly requests: every CLI command that needs the backend, and opening the desktop, including reopening it. Observation, namely `flint status` and the desktop's status polling, never starts one, so a stop holds until the next explicit request. Discovery identifies host processes, including batch-mode editors, and excludes recognized internal workers; a visible window does not make a process a host candidate.

CLI commands and Tauri IPC call the product API. They serialize its results through serde; protocol responses keep their generated types, including their execution status serialization. The CLI owns terminal input, output, and exit codes. The desktop owns its window and tray in a separate process, so backend shutdown and restart leave it open. Backend service hosting belongs to the application API and runs without a desktop event loop.

## Desktop interface

React renders the desktop interface from `apps/flint/src` with Cairn, the first-party design-system submodule at `apps/flint/cairn`. Build the view as [Cairn's application guide](../apps/flint/cairn/docs/applications.md) describes: Flint has no stylesheet of its own, and a visual rule, variant, or reusable component the view needs is added to Cairn. Commit Cairn changes in the submodule before updating Flint's recorded submodule commit.

The `gui` xtask suite runs Flint's `lint` script, which applies Cairn's application rules, together with the typecheck and GUI test. The GUI test serves the window's content security policy from `tauri.conf.json` and fails on any violation.

## Workflow records

Workflow files own durable state; live connections and pending requests are transient. Execution records own the facts used by workflow summaries; counts and participating instances are computed when summaries are read, rather than persisted separately. After a backend restart, Bridges reconnect, and interrupted executions have an unknown host outcome and must not be replayed automatically. A workflow file that cannot be read, because it is corrupt, uses an unsupported schema version, or records values its schema does not accept, fails only the operations that need that workflow; the backend still starts and serves the others. Flint has not released a workflow format, so files from older schema versions are unsupported rather than migrated.

## Bridge

Bridge integrations live under `bridges/hosts` and compose reusable capabilities from `bridges/platforms`. A capability that another host on the same platform can use belongs to the platform, even when it currently has one consumer. Hosts own application-specific startup, dispatch, settings storage, teardown, and delivery. Platform components depend on capabilities supplied by the host rather than selecting concrete hosts. Package loading and external attach converge on the same host adapter. Source ownership and distribution boundaries are independent.

When changing Python or .NET integration code, follow the corresponding [Python](../bridges/platforms/python/README.md) or [.NET](../bridges/platforms/dotnet/README.md) platform guide for composition and lifecycle contracts. For a concrete host's installation and execution behavior, use its guide in the [Bridge index](../bridges/README.md). Runtime code supports the versions declared by its package, independently of the interpreter used by build tooling.

The native core orchestrates Bridge execution, so orchestration changes belong in the core. Its single active record decides when an execution is prepared, run, cancelled, or discarded; it combines output and reports the terminal result, classifying a failure that carries no host code by the stage that failed. A host supplies only its [execution capabilities](../CONTEXT.md): a scheduler that runs callbacks on its execution thread, and an executor that prepares and runs code. The core calls hosts from its dispatcher thread or inside steps the host runs, never from the network runtime.

A language binding of the Bridge core only translates its C ABI: callbacks into the language's values, futures, exceptions, and object lifetimes, creation error codes and command results into the language's own errors, and the core's status snapshot unchanged. It keeps no queue, stage, or connection state, and adds only the failures it alone can detect, such as an unavailable library, so every runtime reports the same Bridge in the core's words. A failed step passes the exception message and stack trace and leaves the code to the core. A host that cannot create its Bridge logs the reason and still offers its settings UI, where applying settings starts a Bridge.

The [C ABI header](../crates/flint-bridge-core/include/flint_bridge_core.h) owns the exact callback contract, and `flint_bridge_abi_version` reports its version. A change to a signature or to a callback's meaning raises that version together with the version every binding expects. The closed creation categories belong to the ABI and are mapped from Rust errors in `ffi.rs`, so Rust error variants change without changing ABI codes.

Stopping a Bridge core ends that core's lifetime as a connection worker. The core rejects configuration and reconnection after stopping, including attempts to reuse it through the manager. Disabling the connection through settings instead retains a reusable core and execution context. Ordinary disconnect requests shutdown without forcibly terminating active host code; while that code still runs, disconnect returns false and the Bridge retains its core and process claim. The caller lets execution finish and retries disconnect to release the resources before creating another Bridge. Disconnect acts on the existing Bridge; queued attach initialization has its own timeout and cancellation, independent of disconnect.

## Errors

A Rust error type exists for the code that handles it. Define a `thiserror` type when a caller branches on its variants, and return that type from the API those callers use. Each variant names a condition some caller treats differently; failures every caller handles alike share one variant. Other errors propagate with `anyhow`, with context naming the operation and its subject. Expected outcomes of an operation, such as a busy instance or a failed execution, are ordinary values or protocol data.

An error's `Display` describes only its own layer, as a lowercase phrase without trailing punctuation, and its cause is reached through `source()`. A report formats the whole chain, for example `{:#}` on `anyhow::Error`, so each cause appears once.

Flint components report failures to one another and to users as `Failure { code, message }`. The code is a stable, nonempty, open string naming the condition rather than where it was detected, so one condition carries one code on every surface. The catalog in `flint-contracts::protocol::failure` lists the codes Flint emits with their default descriptions; hosts may define their own, and every component preserves codes it does not know. Within a component, code classifies a failure by its error type; across a component boundary, by its code. The message is diagnostic text for people and may change; a traceback travels separately.

Errors become `Failure` where they cross a component boundary: in the backend's control responses and execution records, and in the product API, whose callers are CLI and Tauri commands. The crate that owns an error type owns its mapping to codes, unless it has no wire contract, as `flint-hosts`, in which case the application maps it. The product API selects among those mappings and reports anything else as `command_failed`, so a condition its callers must tell apart needs its own error type and code. CLI and Tauri commands report the API's `Failure` unchanged; the CLI adds only failures it detects itself, such as invalid arguments or interruption. An operation that cannot complete fails through its surface's failure channel: a CLI command prints `{ "error": { "code": ..., "message": ... } }` and exits nonzero, and a Tauri command rejects with the same object. A completed operation whose result records a failure, such as an execution, carries the object in the record's `error` field.

Foreign input is validated and normalized once, where it enters: the C ABI normalizes host values, and the store validates workflow records as it reads them. Internal code and outgoing messages therefore hold only values their own contracts accept. Every wire peer is built against the same contracts, so framing checks only the envelope: protocol version, request identity, and a payload the receiver recognizes. A receiver still checks what its own state requires, such as a result that matches a pending execution.

## Protocol and builds

Each boundary between separately built parts has one source, and its other side receives the contract from that source:

| Boundary | Source | Other side receives it as |
|---|---|---|
| Wire protocol: backend, control client, Bridge core | Schemas in `protocol/` | prost bindings in `flint-contracts::protocol` |
| Tauri IPC: executable and React | Rust types that commands accept and return | TypeScript in `apps/flint/src/generated` |
| Bridge core C ABI: core and language bindings | The header, as described under [Bridge](#bridge) | Hand-written bindings, verified by conformance tests |
| Attach: injector and bootstrap | Rust types in `flint-contracts::attach` | The same types, through the dependency |

Desktop commands and events are registered once in `apps/flint/src-tauri/src/desktop/ipc.rs` through tauri-specta. IPC is internal to the desktop module, which exposes only a binding export entry point to the generator. The desktop uses that registry for dispatch, and `cargo codegen` exports its Specta types, commands, events, and constants to `apps/flint/src/generated/bindings.ts`. React calls the generated commands; its backend adapter owns only presentation behavior such as error wrapping and snapshot identity. Frontend type aliases refer to generated types rather than restating their fields. Protocol types exposed through IPC keep their protobuf source: the prost generator adds Specta metadata, including the enum representation of status fields serialized through Serde adapters. Serde omission, flattening, and serialization phases determine the JSON types.

The code generator first synchronizes protobuf Rust output, then compiles the application's `flint-bindings` tool with `--no-default-features --features bindings`. This uses the same product API and IPC registry without building or embedding the frontend that it generates. The default `desktop` feature builds the executable and its frontend.

A type belongs in a protobuf schema only when it travels on the wire; `HostKind` and IPC types stay Rust-sourced. Protocol changes start in the owning schema, with wire semantics beside the corresponding fields and framing implementation. Generated files come only from [`tools/contracts-codegen`](../tools/contracts-codegen/src/main.rs): `cargo codegen` regenerates them in the same change as their sources, and `cargo codegen --check` verifies that they match. A change to the shape or meaning of a wire message raises `PROTOCOL_VERSION` in the framing module, and a change to workflow records raises the workflow schema version in `flint-backend::store`.

Builds use declared, locked dependencies. `cargo xtask setup` owns checkout initialization: repository dependencies and Git hooks. Build, check, and test commands use that prepared environment; application build scripts compile and embed prepared sources. Tooling declares the interpreters it requires but never provisions them; a missing prerequisite fails with an error naming the requirement.

Bridge files reach the executable as a flat set keyed by path, all embedded by `flint-hosts` at compile time: the Python library, the .NET binding, and the Unity package sources, together with the binaries its build compiles from the locked workspace, namely the native Bridge core and, on Windows, the attach bootstrap and the Unity adapter. Those nested builds use their own target directories under the build's output. The bootstrap links the C runtime statically so it needs no runtime present in the target host.

Files gain structure only when they leave the executable, through a [host layout](../CONTEXT.md). Each `HostKind` declares its layouts in `flint-hosts`: an install layout that `flint bridge export` writes in the host's native format, and an attach layout that attach stages for the bootstrap together with its entry. One assembler in `flint-hosts` writes every layout, so a host changes its files or format through its declaration. A host builds on its [platform](../CONTEXT.md): the platform supplies its library with the native core and its attach entry, which it turns into a runtime plan, and the host adds only its own files and entry parameters. A platform's attach entry is written in the platform's language, as `flint_bridge.attach` and `Flint.Bridge.Attach`; Rust passes it a JSON request and adds only what the runtime needs to reach it, such as the Python import root. Processes that host a platform directly use the platform library, which `flint bridge export` writes by platform name.

Attach and export are classified in three layers, each named in its types: host (`HostKind`), platform (`Platform`), and runtime (`RuntimePlan`). Data a layer carries for one use is a data enum named for its layer, such as `PlatformEntry`, and behavior that depends only on the layer is a method on its fieldless enum. When a data enum covers every variant of its layer, it declares the fieldless enum through strum's `EnumDiscriminants`, so the variants have one source. Files a host's own tools read, such as Unity `.meta` files, are committed beside their sources, with GUIDs that stay fixed across releases.

## Formatting and lints

`rust-toolchain.toml` pins Rust and installs rustfmt and Clippy. `rustfmt.toml` selects the 2024 formatting style and a 120-column target width; run `cargo fmt --all` to format workspace sources. The workspace Clippy policy lives in `Cargo.toml`, and every member inherits it through `[lints] workspace = true`. It enables the default `clippy::all` group; additional opinionated groups are not enabled.

Run `cargo xtask test lint` to check formatting and run Clippy on every workspace target with all features enabled. Warnings fail this check. The `lint` suite is part of the default `cargo xtask test` command used by CI and uses `target/lint` for build artifacts. Fix diagnostics at their source; a lint exception belongs at the smallest applicable scope with its reason. Generated Rust remains owned by `cargo codegen` rather than hand-edited to satisfy a check.

`cargo xtask check` runs the format and lint checks without tests or dependency preparation. Select languages with `cargo xtask check rust gui python csharp`. GUI, Python, and C# test suites run their language checks before testing. Rust tests and Rust checks are separate suites, `rust` and `lint`; both run by default.

| Language | Formatter | Lint |
|---|---|---|
| Rust | rustfmt, 2024 style and 120 columns | workspace Clippy rules, warnings are errors |
| TypeScript / JavaScript / CSS | Prettier, 120 columns | ESLint and Stylelint with Cairn's application rules |
| Python | Ruff, 120 columns | Ruff's `E4`, `E7`, `E9`, and `F` rules; runtime Python support follows `bridges/pyproject.toml` |
| C# | `dotnet format` and `.editorconfig` | SDK analyzers and compiler warnings, checked with the locked `bridges/Flint.slnx` projects |

Generated frontend bindings and the Cairn submodule are excluded from application formatting. C# files outside the .NET projects, namely Unity UPM Editor sources and product-test fixtures, receive whitespace checks; their compilation remains with the Unity and runtime tests. `.gitattributes` keeps text files on LF across platforms. Tool versions come from the Rust toolchain, npm lockfile, uv lockfile, and .NET SDK declaration.

Setup installs prek and registers the checkout's pre-commit hook. [prek.toml](../prek.toml) selects language checks from staged paths. The runner temporarily hides unstaged tracked changes and restores them after checking; each selected check validates its project and only reports problems, without formatting or staging files. Hooks use the prepared environment and run the same language checks as CI. Run `cargo xtask check hooks` to apply the hook configuration to every tracked file.

## Tests

Run `cargo xtask test` from the repository root; CI runs the same command. Select suites by name when needed, for example `cargo xtask test rust gui`. Every automated suite, including a new platform's, is registered in [xtask](../tools/xtask/src/main.rs), and test prerequisites and execution details live in the test drivers and configuration. The product integration tests use a dedicated application build with an isolated runtime. The test driver selects this build and keeps its artifacts separate from normal application builds; test processes never fall back to the user's backend. See the runtime configuration in `flint-backend::config` and the shared fixtures in `tests/support.rs` for the isolation contract.

Test behavior at the lowest layer that owns it, then check one representative path through higher layers instead of repeating the lower layer's cases. A test of one Rust crate belongs beside its implementation in that crate's `src`, following Rust's module layout: `src/<module>/tests.rs` for a module or `src/tests.rs` for the crate root. Platform-local tests cover language resource cleanup, marshaling, and execution and dispatch mechanisms, using the language's own test runner and locked dependencies. Python platform tests live beside `src`; .NET projects under `bridges/platforms/dotnet/src` have sibling `<Project>.Tests` projects under `bridges/platforms/dotnet/tests`. Host adapters that require an application's runtime are covered by real-host tests.

The repository-root `tests` directory holds only product integration tests that need the built executable or an exported Bridge package. The application registers that directory as one Rust `product` test target: `tests/main.rs` declares a module for each product area, and files within an area separate its concerns. Tests in `tests/runtime` load the Python and C# exports in disposable language runtimes through their matching xtask suites. Shared Bridge assertions belong to [conformance.rs](../tests/runtime/conformance.rs) and run through both native bindings and platform managers; platform-local tests cover only mechanisms beyond these shared scenarios. Each platform supplies drivers that translate calls and report the production implementation's results. Controlled host executors and dispatchers let the shared judge hold execution or delay initialization while the production manager, Bridge, and core handle their lifecycle. Real-host package scenarios live in `tests/hosts/package`, and injection scenarios in `tests/hosts/injection`. Foreign-language scripts in these tests perform runtime or host-side operations as fixtures; Rust owns orchestration.

Real-host tests are ignored by default and run with `cargo xtask test hosts`. Set `FLINT_MAYA_EXE`, `FLINT_MAX_EXE`, `FLINT_BLENDER_EXE`, and `FLINT_UNITY_EXE` to installed host executables. To run one package scenario, use for example `cargo test --locked --target-dir target/tests -p flint --features test-runtime --test product -- hosts::package::maya --ignored`. Rust-driven Python host tests use the interpreter uv discovers unless `FLINT_TEST_PYTHON` selects one explicitly. Host-test evidence is written under the test build's `target/tests/tmp/host-evidence`.

Starting a real host is expensive, so each host test can verify several related behaviors with explicit checkpoints. Real-host verification uses fresh unsaved processes and cleans up only processes created by the test. Changes to host bootstrap, dispatch, transport, or bundled dependencies require corresponding host verification.
