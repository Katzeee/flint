# .NET Bridge binding

`Flint.Bridge` is a Windows `netstandard2.0` binding for the shared native connection core. `NativeBridge` loads the supplied DLL path and exposes connection state, execute-event polling, result submission, reconnection, and shutdown through a C ABI. The binding does not choose Unity's execution thread or run C# code: a host adapter polls on a worker thread, dispatches execution through the host's lifecycle, then submits output and completion to the core.

The native DLL is built from `crates/flint-bridge-core`. Host packaging must supply the matching DLL and pass its absolute path to `NativeBridge`. A host can keep its own managed bootstrap while sharing the same wire protocol and connection behavior as the Python Bridge.
