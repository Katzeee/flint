# Development guide

This guide covers implementation ownership and test placement. Prepare a Windows checkout and verify the first build with the [environment guide](development.md).

## Ownership

Each Rust crate owns one concept from the [glossary](../CONTEXT.md). Place a feature in the crate that owns its concept, not in the crate that happens to call it:

| Crate | Owns | Depends on |
|---|---|---|
| `crates/flint-contracts` | Shared contracts between Flint components; the `protocol` module owns wire messages, framing, and heartbeat timing, with message schemas in `protocol/` | — |
| `crates/flint-config` | Local backend deployment conventions: control and Bridge endpoints, state directory, and runtime lock files | — |
| `crates/flint-backend` | The backend service: control and Bridge listeners, request dispatch, connected instances, executions, and workflow records | contracts, config |
| `crates/flint-control-client` | Control client requests and the local backend lifecycle: start, stop, and restart | contracts, config |
| `crates/flint-bridge-core` | Bridge core and its C ABI for host adapters, and the process claim that limits a host process to one Bridge | contracts |
| `crates/flint-bridge-bootstrap` | The library flint injects to attach a Bridge into a running host: it drives the host runtime to start the Bridge | — |
| `crates/flint-hosts` | Local host candidates: process discovery, window information, window previews, window focus, and injecting the attach bootstrap | contracts |
| `apps/flint/src-tauri` | The executable: CLI commands, Tauri window, tray, and IPC that compose the crates above | all crates except bridge-core and bridge-bootstrap |

Dependencies point from the executable toward shared contracts and never between peers: the backend and control client share only contracts and config, and the Bridge core never depends on the backend. A feature that needs a new dependency between crates signals misplaced ownership; move the shared concept down instead. Contracts contain shared definitions and their encoding rules; host discovery, resource preparation, runtime drivers, and application behavior belong to their implementing crates. Keep Tauri and UI dependencies out of every crate under `crates`.

The Rust backend owns execution coordination and durable workflow records. Tauri owns the window, tray, and native IPC, while React renders the desktop interface. CLI invocations communicate with the backend through the control client.

Application capabilities and data must be reachable through the CLI. The desktop adapts shared services for presentation rather than owning application behavior. Local host discovery, window previews, and window focus belong to `flint-hosts`, which both CLI and Tauri call without requiring a Bridge or backend connection. Discovery identifies host processes, including batch-mode editors, and excludes recognized internal workers; the presence of a visible window does not define a host candidate.

The React desktop interface lives in `apps/flint/src`, with its npm project and build output beside `src-tauri`. Cairn is the first-party design-system submodule at `apps/flint/cairn`. Build reusable visual components and tokens in Cairn, then consume them in Flint's application-specific interface. Commit Cairn changes in the submodule before updating Flint's recorded submodule commit.

Build the desktop view as [Cairn's application guide](../apps/flint/cairn/docs/applications.md) describes; Flint has no stylesheet of its own. When the view needs a visual rule or variant Cairn lacks, add it to Cairn. Flint's `lint` script runs Cairn's application rules, and the `gui` xtask suite runs it with the typecheck and GUI test. The GUI test serves the window's content security policy from `tauri.conf.json` and fails on any violation. The frontend entry point imports `@cairn/ui/styles.css` and the `@cairn/ui/themes/forest.css` theme stylesheet.

Workflow files own durable state. Live connections and pending requests are transient. After a backend restart, Bridges reconnect; interrupted executions have an unknown host outcome and must not be replayed automatically.

Bridge integrations live under `bridges/hosts` and compose reusable capabilities from `bridges/platforms`. A capability that another host on the same platform can use belongs to the platform, even when it currently has one consumer. Hosts own application-specific startup, dispatch, settings storage, teardown, and delivery. Platform components depend on capabilities supplied by the host rather than selecting concrete hosts. Package loading and external attach converge on the same host adapter. Source ownership and distribution boundaries are independent.

When changing Python or .NET integration code, follow the corresponding [Python](../bridges/platforms/python/README.md) or [.NET](../bridges/platforms/dotnet/README.md) platform guide for composition and lifecycle contracts. For a concrete host's installation and execution behavior, use its guide in the [Bridge index](../bridges/README.md). Runtime code must support the versions declared by its package, independently of the interpreter used by build tooling.

A language binding of the Bridge core only translates its C ABI. It passes the core's status snapshot through unchanged, maps creation error codes and command results to the language's own errors, and adds only the failures it alone can detect, such as an unavailable library. It keeps no connection state and writes no error text that the core already provides, so that every runtime reports the same Bridge the same way. A host that cannot create its Bridge logs the reason and still offers its settings UI, where applying settings starts a Bridge.

Stopping a Bridge core ends that core's lifetime as a connection worker. The core rejects configuration and reconnection after stopping, including attempts to reuse it through the manager. Disabling the connection through settings instead retains a reusable core and execution context. Ordinary disconnect requests shutdown without forcibly terminating active host code; while that code still runs, disconnect returns false and the Bridge retains its core and process claim. The caller lets execution finish and retries disconnect to release the resources before creating another Bridge. Disconnect acts on the existing Bridge; queued attach initialization has its own timeout and cancellation, independent of disconnect.

## Protocol and builds

Protocol changes start in the owning schema. Keep wire semantics beside the corresponding fields and framing implementation, and update generated bindings with their schemas in the same change. Use the configured generator; generated message files are not hand-edited.

Built-in host identities and their external names belong to [`flint-contracts::host`](../crates/flint-contracts/src/host.rs). Discovery and attach inputs use that type; registered Bridge identifiers remain open strings. The `protocol` feature enables wire support for consumers that need it. `cargo codegen` generates protocol bindings and the frontend host type, and `cargo codegen --check` checks that both match their source definitions. The generator lives in [`tools/contracts-codegen`](../tools/contracts-codegen/src/main.rs).

Builds use declared, locked dependencies and remain independent of developer-local environments. xtask owns dependency preparation; application build scripts compile and package prepared sources. Tooling declares the interpreters it requires but never provisions them; a missing prerequisite fails with an error naming the requirement.

The application build compiles the native Bridge core and, on Windows, the attach bootstrap from the locked workspace, and embeds both in the executable: the core in each Bridge package, and the bootstrap for injection. The bootstrap links the C runtime statically so it needs no runtime present in the target host.

## Tests

Run `cargo xtask test` from the repository root; CI runs the same command. Select suites by name when needed, for example `cargo xtask test rust gui`. The product integration tests use a dedicated application build with an isolated runtime. The test driver selects this build and keeps its artifacts separate from normal application builds; test processes never fall back to the user's backend. See the runtime configuration in `flint-config` and the shared fixtures in `tests/support.rs` for the isolation contract. Every automated suite is registered in [xtask](../tools/xtask/src/main.rs). Keep test prerequisites and execution details in the test drivers and configuration.

Place a new test by the behavior it exercises. A test of one Rust crate belongs beside its implementation in that crate's `src`, following Rust's module layout: `src/<module>/tests.rs` for a module or `src/tests.rs` for the crate root. Platform-local tests cover language resource cleanup, marshaling, and execution and dispatch mechanisms, using the language's own test runner and locked dependencies. Python platform tests live beside `src`; .NET projects under `bridges/platforms/dotnet/src` have sibling `<Project>.Tests` projects under `bridges/platforms/dotnet/tests`. Host adapters that require an application's runtime are covered by real-host tests. Register a new platform's suite in xtask.

The repository-root `tests` directory holds only product integration tests that need the built executable or an exported Bridge package. The application registers that directory as one Rust `product` test target: `tests/main.rs` declares a module for each product area, and files within an area separate its concerns. Tests in `tests/runtime` load the Python and C# exports in disposable language runtimes through their matching xtask suites. Shared Bridge assertions belong to [conformance.rs](../tests/runtime/conformance.rs) and run through both native bindings and platform managers. Each platform supplies drivers that translate calls and report the production implementation's results. Controlled host executors and dispatchers let the shared judge hold execution or delay initialization while the production manager, Bridge, and core handle their lifecycle. Platform-local tests cover mechanisms beyond these shared scenarios rather than repeating their assertions. Real-host package scenarios live in `tests/hosts/package`. Injection scenarios live in `tests/hosts/injection`. Foreign-language scripts in these tests perform runtime or host-side operations as fixtures; Rust owns orchestration.

Test behavior at the lowest layer that owns it, then check one representative path through higher layers instead of repeating the lower layer's cases. Name test files for the modules they exercise and keep shared setup in fixtures. Starting a real host is expensive, so each host test can verify several related behaviors with explicit checkpoints.

Real-host tests are ignored by default and run with `cargo xtask test hosts`. Set `FLINT_MAYA_EXE`, `FLINT_MAX_EXE`, `FLINT_BLENDER_EXE`, and `FLINT_UNITY_EXE` to installed host executables. To run one package scenario, use for example `cargo test --locked --target-dir target/tests -p flint --features test-runtime --test product -- hosts::package::maya --ignored`. Rust-driven Python host tests use the interpreter uv discovers unless `FLINT_TEST_PYTHON` selects one explicitly. Host-test evidence is written under the test build's `target/tests/tmp/host-evidence`.

Real-host verification uses fresh unsaved processes and cleans up only processes created by the test. Changes to host bootstrap, dispatch, transport, or bundled dependencies require corresponding host verification.
