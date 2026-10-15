# Unity Bridge

Use Flint to run C# code in Unity Editor on Windows x64 with Mono.

## Install and connect

Run `flint bridge export unity` and install the resulting `flint-unity.tgz` in Unity's Package Manager with **Add package from tarball**. The installed package connects from the Editor to the local backend. Open **Window > Flint Bridge > Connection Settings** for the shared [connection settings](../../README.md#connection-settings). If the Bridge cannot start, the reason appears in the Console; resolve it and use **Connect** to retry.

To connect without installing the package, use `flint apps` to find the Editor's PID, then run `flint attach --pid <pid>`.

## Run code

Submit C# statements, for example:

```csharp
Debug.Log(Selection.activeGameObject);
```

`System`, `UnityEngine`, and `UnityEditor` are available without adding `using` directives. Full source files and async scripts are not supported. Logs written by the submitted code appear in its output; later callbacks' logs are not included.

Keep the Editor open and responsive. Requests wait while Unity compiles scripts. After scripts reload or the backend starts again, Flint reconnects automatically unless you disconnect it. Interrupted code is not rerun automatically; check its effects before submitting it again.
