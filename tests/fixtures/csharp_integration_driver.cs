using System;
using System.Collections.Concurrent;
using System.Text;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using System.IO;
using Flint.Bridge;

// Only the host executor and dispatcher are controlled here. Rust judges the
// exported manager, Bridge and core; this fixture implements no lifecycle policy.
internal static class Program
{
    private static readonly ManualResetEventSlim started = new ManualResetEventSlim();
    private static readonly ManualResetEventSlim release = new ManualResetEventSlim();
    private static readonly BlockingCollection<Action> queued = new BlockingCollection<Action>();
    private static int created, released, finished, factoryThread, dispatchThread;
    private static volatile bool holdDispatch;
    private static Thread attachWorker;
    private static JsonNode attachResult;
    private static BridgeManager manager;
    private static string library;

    private sealed class Scheduler : IExecutionScheduler
    {
        private readonly WorkerThread worker = new WorkerThread();
        public Scheduler() { Interlocked.Increment(ref created); }
        public void Post(Action callback) { worker.Post(callback); }
        public void Dispose() { worker.Dispose(); Interlocked.Increment(ref released); }
    }
    private sealed class Executor : IExecutor
    {
        public Task<object> Prepare(ExecutionRequest request) => Task.FromResult<object>(request);
        public Task Run(object prepared, TextWriter stdout, TextWriter stderr)
        {
            started.Set();
            if (!release.Wait(25000)) throw new TimeoutException("Test did not release the host execution");
            Interlocked.Increment(ref finished);
            return Task.CompletedTask;
        }
    }
    private static ExecutionCapabilities CreateExecution()
    {
        factoryThread = Thread.CurrentThread.ManagedThreadId;
        return new ExecutionCapabilities(new Executor(), new Scheduler());
    }

    private static void RunDispatched(Action callback)
    {
        dispatchThread = Thread.CurrentThread.ManagedThreadId;
        callback();
    }

    private static void Dispatch(Action callback)
    {
        if (holdDispatch) queued.Add(callback);
        else ThreadPool.QueueUserWorkItem(_ => RunDispatched(callback));
    }

    private static BridgeSettings Settings(JsonNode value) => new BridgeSettings
    {
        Address = value["address"].GetValue<string>(),
        Port = value["port"].GetValue<int>(),
        Name = value["name"].GetValue<string>(),
        Enabled = value["enabled"]?.GetValue<bool>() ?? true
    };

    private static JsonNode Call(JsonNode command)
    {
        try
        {
            switch (command["op"].GetValue<string>())
            {
                case "create":
                    manager.Connect(command["library"]?.GetValue<string>() ?? library, Settings(command["config"]));
                    return new JsonObject { ["created"] = true };
                case "claim":
                    using (var core = new NativeCore(library, command["config"].ToJsonString(),
                        new ExecutionCapabilities(new Executor(), new CallbackQueue()))) { }
                    return new JsonObject { ["created"] = true };
                case "apply":
                    manager.Configure(Settings(command["settings"]));
                    return new JsonObject { ["applied"] = true };
                case "attach":
                    return new JsonObject
                    {
                        ["instance_id"] = manager.Attach(library, Settings(command["settings"]),
                        command["timeout_ms"]?.GetValue<int>() ?? 20000)
                    };
                case "status": return manager.StatusJson == null ? null : JsonNode.Parse(manager.StatusJson);
                case "reconnect":
                    manager.Reconnect();
                    return new JsonObject { ["reconnected"] = true };
                case "close": return new JsonObject { ["closed"] = manager.Disconnect() };
                case "probe":
                    return new JsonObject
                    {
                        ["created"] = created,
                        ["released"] = released,
                        ["finished"] = finished,
                        ["factory_thread"] = factoryThread,
                        ["dispatch_thread"] = dispatchThread
                    };
                case "wait_started": return new JsonObject { ["started"] = started.Wait(5000) };
                case "release":
                    release.Set();
                    return new JsonObject { ["released"] = true };
                case "attach_begin":
                    holdDispatch = true;
                    attachResult = null;
                    var attempt = command.DeepClone();
                    attempt["op"] = "attach";
                    attachWorker = new Thread(() => attachResult = Call(attempt)) { IsBackground = true };
                    attachWorker.Start();
                    if (!queued.TryTake(out var initialize, 5000)) throw new TimeoutException("Attach did not dispatch initialization");
                    queued.Add(initialize);
                    return new JsonObject { ["queued"] = true };
                case "attach_result":
                    if (!attachWorker.Join(5000)) throw new TimeoutException("Attach did not return");
                    return attachResult;
                case "drain":
                    holdDispatch = false;
                    while (queued.TryTake(out var callback)) RunDispatched(callback);
                    return new JsonObject { ["drained"] = true };
                default: throw new InvalidOperationException("Unknown driver operation");
            }
        }
        catch (BridgeStoppedException) { return new JsonObject { ["rejected"] = "stopped" }; }
        catch (BridgeBusyException) { return new JsonObject { ["rejected"] = "busy" }; }
        catch (BridgeCreationException error)
        {
            var kind = error.Kind == BridgeCreationErrorKind.Claimed ? "claimed" :
                error.Kind == BridgeCreationErrorKind.LibraryUnavailable ? "library_unavailable" : error.Kind.ToString();
            return new JsonObject { ["error"] = new JsonObject { ["kind"] = kind, ["message"] = error.Message } };
        }
        catch (ArgumentException) { return new JsonObject { ["rejected"] = "invalid_settings" }; }
        catch (Exception error)
        {
            return new JsonObject { ["error"] = new JsonObject { ["type"] = error.GetType().Name, ["message"] = error.Message } };
        }
    }

    private static void Main(string[] args)
    {
        var utf8 = new UTF8Encoding(false);
        Console.InputEncoding = utf8;
        Console.OutputEncoding = utf8;
        library = args[0];
        using (manager = new BridgeManager("standalone_csharp", () => Environment.Version.ToString(), CreateExecution, Dispatch))
        {
            try
            {
                string line;
                while ((line = Console.ReadLine()) != null)
                    Console.WriteLine(Call(JsonNode.Parse(line))?.ToJsonString() ?? "null");
            }
            finally { release.Set(); }
        }
    }
}
