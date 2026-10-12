# Unity Bridge

The Unity Bridge runs submitted C# code inside Unity Editor on Windows x64 with Mono.

## Install and connect

Run `flint bridge export unity` and install the resulting `flint-unity.tgz` in Unity's Package Manager with **Add package from tarball**. The installed package connects from the Editor to the local backend. Open **Window > Flint Bridge > Connection Settings** for the shared [connection settings](../../README.md#connection-settings). If the Bridge cannot start, the reason appears in the Console; resolve it and use **Connect** to retry.

## Connection and execution

Submitted code is a synchronous C# method body, with `System`, `UnityEngine`, and `UnityEditor` available; full source files and async scripts are not supported. It executes on the Editor main thread. Unity logs emitted during the request while its execution context is active are recorded as output; logs from callbacks without that context or after the method returns are not attributed to it. Compilation errors fail the execution as `preparation_failed` and thrown exceptions as `execution_failed`. The Bridge reconnects after a backend restart, but interrupted code is not replayed automatically.

Keep the Editor loop running while requests compile and execute. Requests wait while Unity is already compiling. Assemblies loaded by submitted requests remain loaded until the scripting domain unloads.

External attach and the Editor package provide the same execution and connection behavior. Attaching an Editor with the package already connected reuses its Bridge; applying different settings can produce a new instance ID. When the scripting domain unloads, the Bridge releases its resources and unfinished work does not continue in the new domain. The installed package starts it again after reload; an externally attached Editor can be attached again.
