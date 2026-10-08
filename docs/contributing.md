# Development guide

Conventions for where code belongs, how errors and protocols evolve, how builds stay reproducible, and where tests go. Prepare a Windows checkout and verify the first build with the [environment guide](development.md).

## Ownership

Each Rust crate owns one concept from the [glossary](../CONTEXT.md). Place a feature in the crate that owns its concept, not in the crate that happens to call it:

| Crate | Owns | Depends on |
|---|---|---|
| `crates/flint-contracts` | Shared contracts between Flint components; the `protocol` module owns wire messages, framing, heartbeat timing, and the failure code catalog, with message schemas in `protocol/` | — |
| `crates/flint-config` | Local backend deployment conventions: control and Bridge endpoints, state directory, and runtime lock files | — |
| `crates/flint-backend` | The backend service: control and Bridge listeners, request dispatch, connected instances, executions, and workflow records | contracts, config |
| `crates/flint-control-client` | Control client requests and the local backend lifecycle: start, stop, and restart | contracts, config |
| `crates/flint-bridge-core` | Bridge core and its C ABI for host adapters, and the process claim that limits a host process to one Bridge | contracts |
| `crates/flint-bridge-bootstrap` | The library flint injects to attach a Bridge into a running host: it drives the host runtime to start the Bridge | — |
| `crates/flint-hosts` | Local host candidates: process discovery, window information, window previews, window focus, and injecting the attach bootstrap | contracts |
| `apps/flint/src-tauri` | The executable: CLI commands, Tauri window, tray, and IPC that compose the crates above | all crates except bridge-core and bridge-bootstrap |

Dependencies point from the executable toward shared contracts and never between peers: the backend and control client share only contracts and config, and the Bridge core never depends on the backend. A feature that needs a new dependency between crates signals misplaced ownership; move the shared concept down instead. Contracts hold shared definitions and their encoding rules, while behavior stays in the implementing crates. Tauri and UI dependencies stay in `apps/flint`.

Every application capability and its data are reachable through the CLI, which talks to the backend through the control client; the desktop adapts the same services for presentation. CLI and Tauri call `flint-hosts` directly, without a Bridge or backend connection. Discovery identifies host processes, including batch-mode editors, and excludes recognized internal workers; a visible window does not make a process a host candidate.

## Desktop interface

React renders the desktop interface from `apps/flint/src` with Cairn, the first-party design-system submodule at `apps/flint/cairn`. Build the view as [Cairn's application guide](../apps/flint/cairn/docs/applications.md) describes: Flint has no stylesheet of its own, and a visual rule, variant, or reusable component the view needs is added to Cairn. Commit Cairn changes in the submodule before updating Flint's recorded submodule commit.

The `gui` xtask suite runs Flint's `lint` script, which applies Cairn's application rules, together with the typecheck and GUI test. The GUI test serves the window's content security policy from `tauri.conf.json` and fails on any violation.

## Workflow records

Workflow files own durable state; live connections and pending requests are transient. After a backend restart, Bridges reconnect, and interrupted executions have an unknown host outcome and must not be replayed automatically. A workflow file that cannot be read, because it is corrupt or uses an unsupported schema version, fails only the operations that need that workflow; the backend still starts and serves the others. Flint has not released a workflow format, so files from older schema versions are unsupported rather than migrated.

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

Flint components report failures to one another and to users as `Failure { code, message }`. The code is a stable, nonempty, open string naming the condition rather than where it was detected, so one condition carries one code on every surface. The catalog in `flint-contracts::protocol::failure` lists the codes Flint emits with their default descriptions; hosts may define their own, and every component preserves codes it does not know. Code classifies a failure by its error type within Rust and by its code across a boundary. The message is diagnostic text for people and may change; a traceback travels separately.

Errors become `Failure` where they leave Rust: control responses, CLI output, Tauri commands, and execution records. The crate that owns an error type owns its mapping to codes, unless it has no wire contract, as `flint-hosts`, in which case the application maps it; the application boundary selects among those mappings and reports anything else as `command_failed`. An operation that cannot complete fails through its surface's failure channel: a CLI command prints `{ "error": { "code": ..., "message": ... } }` and exits nonzero, and a Tauri command rejects with the same object. A completed operation whose result records a failure, such as an execution, carries the object in the record's `error` field.

Foreign input is validated and normalized once, where it enters through the C ABI, the wire, or a file, so internal code and outgoing messages hold only values their own contracts accept.

## Protocol and builds

Protocol changes start in the owning schema. Keep wire semantics beside the corresponding fields and framing implementation. Generated files come only from [`tools/contracts-codegen`](../tools/contracts-codegen/src/main.rs): `cargo codegen` regenerates protocol bindings and the frontend host type in the same change as their sources, and `cargo codegen --check` verifies that they match. A change to the shape or meaning of a wire message raises `PROTOCOL_VERSION` in the framing module, and a change to workflow records raises the workflow schema version in `flint-backend::store`.

Built-in host identities and their external names belong to [`flint-contracts::host`](../crates/flint-contracts/src/host.rs). Discovery and attach inputs use that type; registered Bridge identifiers remain open strings. The `protocol` feature enables wire support for consumers that need it.

Builds use declared, locked dependencies and remain independent of developer-local environments. xtask owns dependency preparation; application build scripts compile and package prepared sources. Tooling declares the interpreters it requires but never provisions them; a missing prerequisite fails with an error naming the requirement.

The application build compiles the native Bridge core and, on Windows, the attach bootstrap from the locked workspace, and embeds both in the executable: the core in each Bridge package, and the bootstrap for injection. The bootstrap links the C runtime statically so it needs no runtime present in the target host.

## Tests

Run `cargo xtask test` from the repository root; CI runs the same command. Select suites by name when needed, for example `cargo xtask test rust gui`. Every automated suite, including a new platform's, is registered in [xtask](../tools/xtask/src/main.rs), and test prerequisites and execution details live in the test drivers and configuration. The product integration tests use a dedicated application build with an isolated runtime. The test driver selects this build and keeps its artifacts separate from normal application builds; test processes never fall back to the user's backend. See the runtime configuration in `flint-config` and the shared fixtures in `tests/support.rs` for the isolation contract.

Test behavior at the lowest layer that owns it, then check one representative path through higher layers instead of repeating the lower layer's cases. A test of one Rust crate belongs beside its implementation in that crate's `src`, following Rust's module layout: `src/<module>/tests.rs` for a module or `src/tests.rs` for the crate root. Platform-local tests cover language resource cleanup, marshaling, and execution and dispatch mechanisms, using the language's own test runner and locked dependencies. Python platform tests live beside `src`; .NET projects under `bridges/platforms/dotnet/src` have sibling `<Project>.Tests` projects under `bridges/platforms/dotnet/tests`. Host adapters that require an application's runtime are covered by real-host tests.

The repository-root `tests` directory holds only product integration tests that need the built executable or an exported Bridge package. The application registers that directory as one Rust `product` test target: `tests/main.rs` declares a module for each product area, and files within an area separate its concerns. Tests in `tests/runtime` load the Python and C# exports in disposable language runtimes through their matching xtask suites. Shared Bridge assertions belong to [conformance.rs](../tests/runtime/conformance.rs) and run through both native bindings and platform managers; platform-local tests cover only mechanisms beyond these shared scenarios. Each platform supplies drivers that translate calls and report the production implementation's results. Controlled host executors and dispatchers let the shared judge hold execution or delay initialization while the production manager, Bridge, and core handle their lifecycle. Real-host package scenarios live in `tests/hosts/package`, and injection scenarios in `tests/hosts/injection`. Foreign-language scripts in these tests perform runtime or host-side operations as fixtures; Rust owns orchestration.

Real-host tests are ignored by default and run with `cargo xtask test hosts`. Set `FLINT_MAYA_EXE`, `FLINT_MAX_EXE`, `FLINT_BLENDER_EXE`, and `FLINT_UNITY_EXE` to installed host executables. To run one package scenario, use for example `cargo test --locked --target-dir target/tests -p flint --features test-runtime --test product -- hosts::package::maya --ignored`. Rust-driven Python host tests use the interpreter uv discovers unless `FLINT_TEST_PYTHON` selects one explicitly. Host-test evidence is written under the test build's `target/tests/tmp/host-evidence`.

Starting a real host is expensive, so each host test can verify several related behaviors with explicit checkpoints. Real-host verification uses fresh unsaved processes and cleans up only processes created by the test. Changes to host bootstrap, dispatch, transport, or bundled dependencies require corresponding host verification.
