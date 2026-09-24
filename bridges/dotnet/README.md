# .NET Bridge

The supported .NET host is Unity Editor on Windows x64 with Mono. Unity must load the Bridge to connect; `flint hosts` only discovers the Editor process.

## C# integration

`flint bridge export csharp` writes `flint-csharp.zip` with `NativeBridge.cs` and the native core DLL in one directory. Use these files when integrating the Bridge with another C# host. The host still supplies its own execution-thread dispatch and code runner.

## Unity

Run `flint bridge export unity`, then install the resulting `flint-unity.tgz` in Unity's Package Manager with **Add package from tarball**. Start flint with `flint start`; the installed package connects from the Editor to the default local registry port, `6321`. Open **Window > Flint Bridge > Connection Settings** to inspect the connection, change its address, port, instance name, and enabled state, or reconnect. **Apply** changes the live connection and saves the values in Unity's per-user Editor preferences. Confirm registration with `flint instances --json`.

## Connection and execution

Follow the [flint execution guide](../../README.md#execute-and-inspect) to submit code to the registered instance. Submitted code is a synchronous C# method body, with `System`, `UnityEngine`, and `UnityEditor` available; full source files and async scripts are not supported. It executes on the Editor main thread. Unity logs emitted during the request while its execution context is active are recorded as output; logs from callbacks without that context or after the method returns are not attributed to it. Compilation errors or thrown exceptions fail the execution. The Bridge reconnects after a backend restart, but interrupted code is not replayed automatically.
