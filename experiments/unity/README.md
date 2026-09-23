# Unity execution output scope probe

`output_scope.cs` is a DotCraft Unity C# snippet for an already running Editor. It creates no assets or scene objects. It briefly subscribes to Unity's threaded log event, emits uniquely marked logs, and removes its callback before returning. The probe compares logs from the execution's main-thread entry, a frame continuation, and a `Task.Run` with logs from a task whose `ExecutionContext` flow is suppressed and an unrelated Editor update callback.

Run it against a selected Editor with the [dotcraft-unity CLI](https://github.com/DotHarness/dotcraft-unity), using an explicit project root and PID:

```powershell
dotcraft-unity exec --backend attach --pid <editor-pid> --project-root <unity-project> --path D:\codes\flint\experiments\unity\output_scope.cs --json
```

The result's `seen` entries include the `AsyncLocal` scope and managed thread ID observed inside `Application.logMessageReceivedThreaded`. On Unity 2022.3.62f1 Mono, execution entry and frame continuation run on main thread 1; their logs and a `Task.Run` log retain the scope, while the suppressed-flow task and Editor update callback report `<none>`. This establishes a usable attribution mechanism for managed logs that retain the execution context. It does not establish attribution for native Unity logs, callbacks detached from that context, or output after the execution ends. The Attach backend returns an empty `logs` array even though the Unity Console receives these messages.
