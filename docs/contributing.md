# Development guide

This guide covers implementation ownership and test placement. Prepare a Windows checkout and verify the first build with the [environment guide](development.md).

## Ownership

Each Rust crate owns one concept from the [glossary](../CONTEXT.md). Place a feature in the crate that owns its concept, not in the crate that happens to call it:

| Crate | Owns | Depends on |
|---|---|---|
| `crates/flint-protocol` | Wire protocol schema, generated messages, framing, and heartbeat timing | — |
| `crates/flint-config` | Local backend deployment conventions: control and registry endpoints, state directory, and runtime lock files | — |
| `crates/flint-backend` | The backend service: control and registry listeners, request dispatch, connected instances, executions, and workflow records | protocol, config |
| `crates/flint-control-client` | Control client requests and the local backend lifecycle: start, stop, and restart | protocol, config |
| `crates/flint-bridge-core` | Bridge core and its C ABI for host adapters | protocol |
| `crates/flint-hosts` | Local host candidates: process discovery, window information, window previews, and window focus | — |
| `apps/flint/src-tauri` | The executable: CLI commands, Tauri window, tray, and IPC that compose the crates above | all crates except bridge-core |

Dependencies point from the executable toward the protocol and never between peers: the backend and control client share only protocol and config, and the Bridge core never depends on the backend. A feature that needs a new dependency between crates signals misplaced ownership; move the shared concept down instead. Keep Tauri and UI dependencies out of every crate under `crates`.

The Rust backend owns execution coordination and durable workflow records. Tauri owns the window, tray, and native IPC, while React renders the desktop interface. CLI invocations communicate with the backend through the control client.

Application capabilities and data must be reachable through the CLI. The desktop adapts shared services for presentation rather than owning application behavior. Local host discovery, window previews, and window focus belong to `flint-hosts`, which both CLI and Tauri call without requiring a Bridge or backend connection. Discovery identifies host processes, including batch-mode editors, and excludes recognized internal workers; the presence of a visible window does not define a host candidate.

The React desktop interface lives in `apps/flint/src`, with its npm project and build output beside `src-tauri`. Cairn is the first-party design-system submodule at `apps/flint/cairn`. Build reusable visual components and tokens in Cairn, then consume them in Flint's application-specific interface. Commit Cairn changes in the submodule before updating Flint's recorded submodule commit.

Build the desktop view as [Cairn's application guide](../apps/flint/cairn/docs/applications.md) describes; Flint has no stylesheet of its own. When the view needs a visual rule or variant Cairn lacks, add it to Cairn. Flint's `lint` script runs Cairn's application rules, and the `gui` xtask suite runs it with the typecheck and GUI test. The GUI test serves the window's content security policy from `tauri.conf.json` and fails on any violation. The frontend entry point imports `@cairn/ui/styles.css` and the `@cairn/ui/themes/forest.css` theme stylesheet.

Workflow files own durable state. Live connections and pending requests are transient. After a backend restart, Bridges reconnect; interrupted executions have an unknown host outcome and must not be replayed automatically.

Keep host-specific runtime code and configuration within the owning subproject. Host adapters own UI-thread dispatch. Runtime code must support the versions declared by its package, independently of the interpreter used by build tooling.

## Protocol and builds

Protocol changes start in the owning schema. Keep wire semantics beside the corresponding fields and framing implementation, and update generated bindings with their schemas in the same change. Use the configured generator; generated message files are not hand-edited.

Builds use declared, locked dependencies and remain independent of developer-local environments. xtask owns dependency preparation; application build scripts compile and package prepared sources. Tooling declares the interpreters it requires but never provisions them; a missing prerequisite fails with an error naming the requirement.

## Tests

Run `cargo xtask test` from the repository root; CI runs the same command. Select suites by name when needed, for example `cargo xtask test rust gui`. Cargo builds the application executable used by the product integration tests, so a separate build is unnecessary. Every automated suite is registered in [xtask](../tools/xtask/src/main.rs). Keep test prerequisites and execution details in the test drivers and configuration.

Place a new test by the behavior it exercises. A test of one Rust crate belongs beside its implementation in that crate's `src`, following Rust's module layout: `src/<module>/tests.rs` for a module or `src/tests.rs` for the crate root. A test of one language Bridge belongs beside that Bridge's package and uses the language's own test runner and locked dependencies. Python packages keep `tests` next to `src`; .NET projects under `bridges/dotnet/src` have sibling `<Project>.Tests` projects under `bridges/dotnet/tests`. Host adapters that require an application's runtime are covered by real-host tests. Register a new language Bridge's suite in xtask.

The repository-root `tests` directory holds only product integration tests that need the built executable or an exported Bridge package. The application registers that directory as one Rust `product` test target: `tests/main.rs` declares a module for each product area, and files within an area separate its concerns. Tests in `tests/runtime` load the raw Python and C# exports in disposable language runtimes; they run through the matching `python` and `csharp` xtask suites. Real-host package scenarios live in `tests/hosts/package`. The `tests/hosts/injection` module reserves the same product-test location for injection scenarios when injection is implemented. Foreign-language scripts in these tests perform runtime or host-side operations as fixtures; Rust owns orchestration.

Test behavior at the lowest layer that owns it, then check one representative path through higher layers instead of repeating the lower layer's cases. Name test files for the modules they exercise and keep shared setup in fixtures. Starting a real host is expensive, so each host test can verify several related behaviors with explicit checkpoints.

Real-host tests are ignored by default and run with `cargo xtask test hosts`. Set `FLINT_MAYA_EXE`, `FLINT_MAX_EXE`, `FLINT_BLENDER_EXE`, and `FLINT_UNITY_EXE` to installed host executables. To run one package scenario, use for example `cargo test --locked -p flint --test product -- hosts::package::maya --ignored`. Rust-driven Python host tests use the interpreter uv discovers unless `FLINT_TEST_PYTHON` selects one explicitly. Host-test evidence is written under Cargo's `target/tmp/host-evidence`.

Real-host verification uses fresh unsaved processes and cleans up only processes created by the test. Changes to host bootstrap, dispatch, transport, or bundled dependencies require corresponding host verification.
