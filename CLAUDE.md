# Project Guidelines

The Rust backend owns execution coordination and durable workflow records. Tauri owns desktop presentation in the backend process; CLI invocations communicate with that process through the control client. Keep Tauri dependencies out of the core and protocol crates.

Workflow files own durable state. Live connections and pending requests are transient; after a backend restart, bridges reconnect, while interrupted executions have an unknown host outcome and must not be replayed automatically.

Each host-side language project owns its configuration: `bridges/python` contains its uv workspace, packages, component tests, and packager; `bridges/dotnet` contains its SDK selection, protocol project, and C# peer. Python Bridge runtime sources remain compatible with Python 3.7. Host adapters own UI-thread dispatch; Python build tooling uses 3.13 through uv.

Protocol changes start in `protocol/flint_protocol/v1`. Keep wire semantics beside the corresponding schema fields and framing implementation. Regenerate bindings with `cargo codegen`, verify them with `cargo codegen --check`, and commit schemas and generated sources together. Generated message files are not hand-edited.

Product integration tests belong to the Rust package `tests/integration` and exercise the built EXE and exported Bridge ZIP. Python files there are disposable host fixtures, not test drivers. Build the application before running `cargo test --workspace`; use FLINT_BINARY to select another build and FLINT_TEST_PYTHON to select the fixture interpreter. Run Python component tests with `uv run --directory bridges/python --locked --package flint-bridge --group test --python 3.13 pytest`. Verification runs through Cargo or the scoped pytest project, rather than ad hoc Python commands.

Cargo builds prepare their own locked Python environment inside OUT_DIR; build inputs must not depend on `_local` or a pre-existing virtual environment. Real Maya/Max tests are explicitly ignored by default and launch fresh unsaved host processes when selected. Clean up only processes created by the test. Run host tests when changing their bootstrap, dispatch, transport, or bundled dependencies.
