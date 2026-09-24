# .NET Bridge

The supported .NET host is Unity Editor on Windows x64, verified with Unity 2022.3.62f1 on Mono. CoreCLR is not yet verified. Unity must load the Bridge to connect; `flint hosts` only discovers the Editor process.

## Prepare the Bridge

Export the Bridge and start flint:

```text
flint bridge export --output flint-bridge.zip
flint start
```

Extract `unity/EditorBridge.cs` and `unity/NativeBridge.cs` from the ZIP into the Unity project's `Assets/Editor/Flint` directory. Extract `flint_bridge/native/flint_bridge_core.dll` to a stable path outside `Assets`, such as `<project>/Flint/flint_bridge_core.dll`. Keep the files from the same export together when updating the Bridge.

## Unity

For startup connection, set `FLINT_UNITY_CORE` to the DLL's absolute path and `FLINT_UNITY_PORT` to the backend registry port before launching the Editor. Set the port to `6321` for flint's default configuration. For an Editor that is already running, call `Flint.Unity.EditorBridge.Connect(nativeLibraryPath, "127.0.0.1", 6321, "My Unity Editor")` from an Editor script on its main thread. Confirm registration with `flint instances --json`.

## Connection and execution

Follow the [flint execution guide](../../README.md#execute-and-inspect) to submit code to the registered instance. Submitted code is a synchronous C# method body, with `System`, `UnityEngine`, and `UnityEditor` available; full source files and async scripts are not supported. It executes on the Editor main thread. Unity logs are recorded as output, and compilation errors or thrown exceptions fail the execution. The Bridge reconnects after a backend restart, but interrupted code is not replayed automatically. The [output-scope experiment](../../experiments/unity/README.md) records findings for future asynchronous execution.
