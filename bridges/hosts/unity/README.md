# Unity Bridge

The Unity Bridge supports Unity Editor on Windows x64 with Mono. The integration owns the Editor package, code execution, connection settings, and the managed entry used by external attach. It uses the [.NET platform](../../platforms/dotnet/README.md) to connect to the native Bridge core.

## Install and connect

Run `flint bridge export unity`, then install the resulting `flint-unity.tgz` in Unity's Package Manager with **Add package from tarball**. Start flint with `flint start`; the installed package connects from the Editor to the default local Bridge port, `6321`. Open **Window > Flint Bridge > Connection Settings** to inspect the connection, change its address, port, instance name, and enabled state, or reconnect. **Apply** changes the live connection and saves the values in Unity's per-user Editor preferences. If the Bridge could not start, for example because another Bridge already owns the Editor process, the reason appears in the Console and **Apply** starts it. Confirm registration with `flint instances --json`.

## Connection and execution

After connecting through the Editor package or external attach, follow the [flint execution guide](../../../README.md#execute-and-inspect) to submit code to the registered instance. Submitted code is a synchronous C# method body, with `System`, `UnityEngine`, and `UnityEditor` available; full source files and async scripts are not supported. It executes on the Editor main thread. Unity logs emitted during the request while its execution context is active are recorded as output; logs from callbacks without that context or after the method returns are not attributed to it. Compilation errors or thrown exceptions fail the execution. The Bridge reconnects after a backend restart, but interrupted code is not replayed automatically.

External attach and the Editor package load the same `Flint.Unity.dll` and enter the same adapter. Both provide code execution, settings updates, and reconnection. Attaching an Editor with the package already connected reuses that adapter; applying different settings can produce a new backend registration. The Bridge releases its resources when the scripting domain unloads. The installed package starts it again after reload; an externally attached Editor can be attached again.
