using System;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using System.Collections.Concurrent;
using System.IO;
using System.Text.Json.Nodes;
using Flint.Bridge;

// Drive the exported C# binding for the runtime conformance scenarios. Reads
// one JSON command per line and answers each with one JSON line. Results are
// reported as the binding produced them; Rust owns every assertion.
internal static class Program
{
    private static NativeCore core;
    private static WorkerThread scheduler;
    private static readonly BlockingCollection<ExecutionRequest> requests = new BlockingCollection<ExecutionRequest>();
    private static readonly ManualResetEventSlim release = new ManualResetEventSlim();
    private sealed class Executor : IExecutor
    {
        public Task<object> Prepare(ExecutionRequest request) => Task.FromResult<object>(request);
        public Task Run(object prepared, TextWriter stdout, TextWriter stderr)
        {
            requests.Add((ExecutionRequest)prepared);
            if (!release.Wait(15000)) throw new TimeoutException("Test did not release execution");
            return Task.CompletedTask;
        }
    }
    private static JsonNode Finish() { release.Set(); return new JsonObject { ["reported"] = true }; }

    private static int Main(string[] args)
    {
        var utf8 = new UTF8Encoding(false);
        Console.InputEncoding = utf8;
        Console.OutputEncoding = utf8;
        var library = args[0];
        string line;
        while ((line = Console.ReadLine()) != null)
        {
            using var document = JsonDocument.Parse(line);
            var command = document.RootElement;
            JsonNode reply = command.GetProperty("op").GetString() switch
            {
                "create" => Create(library, command.GetProperty("config").GetRawText()),
                "apply" => Apply(command.GetProperty("settings").GetRawText()),
                "take" => Take(),
                "finish" => Finish(),
                "close" => Close(),
                "status" => JsonNode.Parse(core.StatusJson),
                var other => throw new InvalidOperationException("Unknown command " + other)
            };
            Console.WriteLine(reply!.ToJsonString());
        }
        return 0;
    }

    private static JsonNode Create(string library, string config)
    {
        var worker = new WorkerThread();
        try
        {
            core = new NativeCore(library, config, new ExecutionCapabilities(new Executor(), worker));
            scheduler = worker;
            release.Reset();
            return new JsonObject { ["created"] = true };
        }
        catch
        {
            worker.Dispose();
            throw;
        }
    }

    private static JsonNode Apply(string settings)
    {
        try
        {
            core.ApplySettings(settings);
            return new JsonObject { ["applied"] = true };
        }
        catch (BridgeBusyException)
        {
            return new JsonObject { ["rejected"] = "busy" };
        }
    }

    private static JsonNode Take()
    {
        if (!requests.TryTake(out var request, 10000)) throw new TimeoutException("No execution entered the host");
        return new JsonObject { ["request_id"] = request.RequestId };
    }

    private static JsonNode Close()
    {
        if (!core.Stop()) return new JsonObject { ["closed"] = false };
        core.Dispose();
        scheduler.Dispose();
        return new JsonObject { ["closed"] = true };
    }
}
