var marker = "FLINT_OUTPUT_SCOPE_" + Guid.NewGuid().ToString("N");
var scope = new System.Threading.AsyncLocal<string>();
var seen = new System.Collections.Concurrent.ConcurrentQueue<string>();
Application.LogCallback callback = (message, stack, type) =>
{
    if (message.StartsWith(marker))
        seen.Enqueue(message.Substring(marker.Length) + ":scope=" + (scope.Value ?? "<none>")
            + ":thread=" + Thread.CurrentThread.ManagedThreadId);
};
Application.logMessageReceivedThreaded += callback;
EditorApplication.CallbackFunction ambient = null;
try
{
    scope.Value = marker;
    var entryThread = Thread.CurrentThread.ManagedThreadId;
    Debug.Log(marker + ":main");
    await ctx.WaitFrame();
    var resumedThread = Thread.CurrentThread.ManagedThreadId;
    Debug.Log(marker + ":after-frame");
    await Task.Run(() => Debug.Log(marker + ":worker"));
    Task withoutScope;
    using (System.Threading.ExecutionContext.SuppressFlow())
        withoutScope = Task.Run(() => Debug.Log(marker + ":no-scope-worker"));
    await withoutScope;
    ambient = () => { EditorApplication.update -= ambient; Debug.Log(marker + ":editor-update"); };
    EditorApplication.update += ambient;
    await ctx.WaitFrames(2);
    return new { entryThread, resumedThread, seen = seen.ToArray() };
}
finally
{
    if (ambient != null) EditorApplication.update -= ambient;
    Application.logMessageReceivedThreaded -= callback;
    scope.Value = null;
}
