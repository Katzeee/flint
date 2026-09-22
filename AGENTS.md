# Development conventions

The Rust backend owns execution coordination and durable workflow records. Tauri owns desktop presentation in the backend process; CLI invocations communicate with that process through the control client. Keep Tauri dependencies out of the core and protocol crates.

Workflow files own durable state. Live connections and pending requests are transient; after a backend restart, bridges reconnect, while interrupted executions have an unknown host outcome and must not be replayed automatically.

Keep host-specific code, configuration, and component tests within the owning language subproject. Host adapters own UI-thread dispatch. Runtime code must support the versions declared by its package, independently of the interpreter used by build tooling.

Protocol changes start in the owning schema. Keep wire semantics beside the corresponding fields and framing implementation, and update generated bindings with their schemas in the same change. Use the configured generator; generated message files are not hand-edited.

Product integration tests are Rust-driven and exercise the built executable and exported Bridge package. Foreign-language scripts in those tests perform host-side operations; they do not own test orchestration. Component tests belong to their language subproject. Keep test prerequisites and execution details in the test drivers and configuration.

Builds must use declared, locked dependencies and remain independent of developer-local environments. Real-host verification uses fresh unsaved processes and cleans up only processes created by the test. Changes to host bootstrap, dispatch, transport, or bundled dependencies require corresponding host verification.

For environment preparation and the first application build, follow [docs/development.md](docs/development.md).
