# Contributing to Flint

Prepare a Windows checkout and verify the first build with the [environment guide](development.md).

This guide records conventions that outlast individual changes: where code belongs, how failures cross boundaries, and how changes are verified. Product behavior lives in the code, the [glossary](../CONTEXT.md), and the [README](../README.md); a change that alters behavior updates those.

## Ownership

Place a type or behavior in the component that defines its meaning, using the [glossary](../CONTEXT.md) to resolve terms; callers depend on that owner. Within a crate, modules and visibility express ownership.

A product crate exists for one of three reasons, named in its row below:

- **Artifact**: it builds into a separate binary.
- **Contract**: it holds what separately built binaries exchange. `flint-contracts` is the only one.
- **Service**: a domain the executable composes, which builds and tests without Tauri.

Other product code belongs in a module of the crate that owns its meaning, including code several crates call. A new product crate adds its row here. Development tools live under `tools/`.

| Crate | Reason | Owns |
|---|---|---|
| `crates/flint-contracts` | Contract | What separately built binaries exchange, and the runtime configuration they share |
| `crates/flint-backend` | Service | The backend service: listeners, request dispatch, connected instances, executions, and workflow records |
| `crates/flint-hosts` | Service | Host discovery and inspection, window operations, Bridge packaging, and bootstrap injection |
| `crates/flint-bridge-core` | Artifact | Bridge core and its C ABI for host adapters |
| `crates/flint-bridge-bootstrap` | Artifact | Starting the platform's Bridge entry inside an injected host runtime |
| `apps/flint/src-tauri` | Artifact | The product API, CLI commands, the control client and local backend lifecycle, Tauri window, tray, and IPC |

Dependencies stay acyclic and follow ownership. Tauri and UI dependencies stay in `apps/flint`; the Bridge core and bootstrap depend only on `flint-contracts`, the bootstrap without its `protocol` feature.

Expose product operations through `application` in `apps/flint/src-tauri`, independently of terminal input and window presentation. CLI and Tauri commands call this API. Use the single runtime configuration entry in `flint-contracts::config` for the application and its backend child; callers supply no per-invocation overrides. Local host inspection, focus, and export remain independent of the backend and its configuration; backend operations use the control protocol.

The CLI owns terminal input, output, and exit codes. Backend service hosting belongs to the application API and runs without a desktop event loop.

Config owns directory policy and cross-process coordination; consumers own other filesystem operations. Bootstrap execution must not depend on the host's directory configuration.

## Desktop interface

React renders the desktop interface from `apps/flint/src` with Cairn, the first-party design-system submodule at `apps/flint/cairn`. Build the view as [Cairn's application guide](../apps/flint/cairn/docs/applications.md) describes: Flint has no stylesheet of its own, and a visual rule, variant, or reusable component the view needs is added to Cairn. Commit Cairn changes in the submodule before updating Flint's recorded submodule commit.

GUI tests use the desktop's content security policy and fail on violations.

## Workflow records

Workflow files own durable state; live connections and pending requests are transient. Execution records own the facts used by workflow summaries; derived values are computed when summaries are read, rather than persisted separately. A workflow file that cannot be read fails only the operations that need that workflow. Flint has not released a workflow format, so files from older schema versions are unsupported rather than migrated.

## Bridge

Bridge integrations live under `bridges/hosts` and compose reusable capabilities from `bridges/platforms`. A capability that another host on the same platform can use belongs to the platform, even when it currently has one consumer. Hosts own application-specific startup, dispatch, settings storage, teardown, and delivery. Platform components depend on capabilities supplied by the host rather than selecting concrete hosts. Package loading and external attach converge on the same host adapter. Source ownership and distribution boundaries are independent.

Hosts persist their host settings; connection state is transient. Bridge creation failures leave the settings UI available and report the reason through the host's diagnostics.

Runtime code supports the versions declared by its package, independently of the interpreter used by build tooling.

Execution orchestration, connection state, and result classification belong to the native core. Hosts supply execution capabilities: scheduling on the required thread and preparing and running code. Keep host callbacks off the network runtime.

Language bindings translate the Bridge core's C ABI into language-native values, asynchronous completions, errors, and lifetimes. Execution and connection state remain in the core; bindings preserve its status snapshot and failure codes and add only failures they detect locally. A failed step passes the exception message and stack trace and leaves classification to the core.

Importing an integration leaves connections and UI untouched. A host integration cannot replace or disconnect another host's Bridge. Disconnect must let running host code finish; ending a connection does not authorize terminating that code.

Implement callbacks and resource release according to the [C ABI header](../crates/flint-bridge-core/include/flint_bridge_core.h). Changes to signatures or callback semantics raise the ABI version and every binding's expected version together. Keep ABI creation categories independent of Rust error variants.

## Errors

Use a `thiserror` type when callers distinguish failure conditions, with one variant per distinct handling case, and return it from the API they call. Otherwise propagate errors with `anyhow`, adding the operation and subject as context. Expected outcomes, such as a busy instance or a failed execution, are ordinary values or protocol data.

An error's `Display` describes only its own layer, as a lowercase phrase without trailing punctuation, and its cause is reached through `source()`. A report formats the whole chain, for example `{:#}` on `anyhow::Error`, so each cause appears once.

Report cross-component failures as `Failure { code, message }`. Codes are stable, nonempty, open strings identifying conditions consistently across surfaces; messages are diagnostic text and may change. Use error types within a component and codes across boundaries. Maintain Flint's codes in `flint-contracts::protocol::failure`; hosts may define their own, and every component preserves unknown codes. Tracebacks travel separately from messages.

Errors become `Failure` where they cross a component boundary: in the backend's control responses and execution records, and in the product API, whose callers are CLI and Tauri commands. The crate that owns an error type owns its mapping to codes, unless it has no wire contract, as `flint-hosts`, in which case the application maps it. The product API selects among those mappings and reports anything else as `command_failed`, so a condition its callers must tell apart needs its own error type and code. CLI and Tauri commands report the API's `Failure` unchanged; the CLI adds only failures it detects itself, such as invalid arguments or interruption.

Operations that cannot complete use the surface's failure channel: the CLI reports the failure and exits nonzero, and Tauri commands reject with it. Completed operations, such as executions, carry any failure in their result record.

Foreign input is validated and normalized once, where it enters: the C ABI normalizes host values, and the store validates workflow records as it reads them. Internal code and outgoing messages therefore hold only values their own contracts accept. Every wire peer is built against the same contracts, so framing checks only the envelope: protocol version, request identity, and a payload the receiver recognizes. A receiver still checks what its own state requires, such as a result that matches a pending execution.

## Protocol and builds

Each boundary between separately built parts has one source, and its other side receives the contract from that source:

| Boundary | Source | Other side receives it as |
|---|---|---|
| Wire protocol: backend, control client, Bridge core | Schemas in `protocol/` | prost bindings in `flint-contracts::protocol` |
| Tauri IPC: executable and React | Rust types that commands accept and return | TypeScript in `apps/flint/src/generated` |
| Bridge core C ABI: core and language bindings | The header, as described under [Bridge](#bridge) | Hand-written bindings, verified by conformance tests |
| Attach: injector and bootstrap | Rust types in `flint-contracts::attach` | The same types, through the dependency |

Register desktop commands and events once in `apps/flint/src-tauri/src/desktop/ipc.rs`. React uses generated commands and types; its backend adapter owns presentation behavior. Keep wire types protobuf-sourced even when exposed through IPC.

A type belongs in a protobuf schema only when it travels on the wire; `HostKind` and IPC types stay Rust-sourced. Protocol changes start in the owning schema, with wire semantics beside the corresponding fields and framing implementation. Generated files come only from [`tools/contracts-codegen`](../tools/contracts-codegen/src/main.rs): `cargo codegen` regenerates them in the same change as their sources, and `cargo codegen --check` verifies that they match. A change to the shape or meaning of a wire message raises `PROTOCOL_VERSION` in the framing module, and a change to workflow records raises the workflow schema version in `flint-backend::store`.

Builds use declared, locked dependencies. `cargo xtask setup` owns checkout initialization: repository dependencies and Git hooks. Build, check, and test commands use that prepared environment; application build scripts compile and embed prepared sources. Tooling declares the interpreters it requires but never provisions them; a missing prerequisite fails with an error naming the requirement.

Declare host installation and attach layouts in `flint-hosts` and use its shared assembler. Platforms supply reusable libraries and attach entries; hosts add their own files and entry parameters. Platforms construct typed runtime plans; bootstrap translates them into host-runtime calls without selecting host integrations. Keep host, platform, and runtime responsibilities separate when adding integrations. Put layer-wide behavior on its enum and use a data enum for use-specific payloads; derive matching discriminants so variants have one source.

Commit files a host's own tools read, such as Unity `.meta` files, beside their sources, with GUIDs stable across releases.

## Formatting and lints

Rust formatting rules live in [rustfmt.toml](../rustfmt.toml); run `cargo fmt --all` to format workspace sources. The workspace Clippy policy lives in [Cargo.toml](../Cargo.toml), and every member inherits it through `[lints] workspace = true`.

Run `cargo xtask test lint` to check formatting and run Clippy on every workspace target with all features enabled. Warnings fail this check. Fix diagnostics at their source; a lint exception belongs at the smallest applicable scope with its reason.

`cargo xtask check` runs the format and lint checks without tests or dependency preparation. Select languages with `cargo xtask check rust gui python csharp`. GUI, Python, and C# test suites run their language checks before testing. Rust tests and Rust checks are separate suites, `rust` and `lint`.

Generated frontend bindings and the Cairn submodule are excluded from application formatting. C# files outside the .NET projects, namely Unity UPM Editor sources and product-test fixtures, receive whitespace checks; their compilation remains with the Unity and runtime tests.

[prek.toml](../prek.toml) selects pre-commit checks from staged paths. Hooks validate projects with the same checks as CI and report problems without formatting or staging files. Run `cargo xtask check hooks` to check every tracked file.

## Tests

Run `cargo xtask test` from the repository root; CI runs the same command, including both `rust` and `lint` suites by default. Select suites by name when needed, for example `cargo xtask test rust gui`. Register every automated suite in [xtask](../tools/xtask/src/main.rs). Product integration tests use the isolated application build selected by the test driver; they must never fall back to the user's backend. Reuse `tests/support.rs` for runtime isolation.

Test behavior at the lowest layer that owns it, then check one representative path through higher layers instead of repeating the lower layer's cases. Crate-local Rust tests live in `src/<module>/tests.rs` or `src/tests.rs`. Platform-local tests cover resource cleanup, marshaling, and execution and dispatch mechanisms with native test runners and locked dependencies. Python tests live beside `src`; .NET test projects live under `bridges/platforms/dotnet/tests/<Project>.Tests`. Host-dependent behavior uses real-host tests.

The repository-root `tests` directory holds product integration tests that need the built executable or an exported Bridge package. Put shared Bridge assertions in [conformance.rs](../tests/runtime/conformance.rs) so native bindings and platform managers face the same scenarios. Platform drivers translate calls and report production results; controlled executors and dispatchers supply timing, while production code handles lifecycle. Platform-local tests cover mechanisms beyond the shared scenarios. Foreign-language scripts are runtime or host-side fixtures; Rust owns product-test orchestration.

Real-host tests are ignored by default and run with `cargo xtask test hosts`. Set `FLINT_MAYA_EXE`, `FLINT_MAX_EXE`, `FLINT_BLENDER_EXE`, `FLINT_HOUDINI_EXE`, and `FLINT_UNITY_EXE` to installed host executables. To run one package scenario, use for example `cargo test --locked --target-dir target/tests -p flint --features test-runtime --test product -- hosts::package::maya --ignored`. Rust-driven Python host tests use the interpreter uv discovers unless `FLINT_TEST_PYTHON` selects one explicitly. Host-test evidence is written under the test build's `target/tests/tmp/host-evidence`.

Starting a real host is expensive, so each host test can verify several related behaviors with explicit checkpoints. Real-host verification uses fresh unsaved processes and cleans up only processes created by the test. Changes to host bootstrap, dispatch, transport, or bundled dependencies require corresponding host verification.
