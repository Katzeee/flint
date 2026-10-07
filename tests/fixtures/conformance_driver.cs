using System;
using System.Collections.Generic;
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
        var library = args[0];
        string line;
        while ((line = Console.ReadLine()) != null)
        {
            var command = JsonNode.Parse(line)!.AsObject();
            JsonNode reply = command["op"]!.GetValue<string>() switch
            {
                "create" => Create(command["library"]?.GetValue<string>() ?? library,
                    command["config"]!.ToJsonString()),
                "apply" => Apply(command["settings"]!.ToJsonString()),
                "take" => Take(),
                "finish" => Finish(),
                "reconnect" => Reconnect(),
                "close" => Close(),
                "status" => JsonNode.Parse(core.StatusJson),
                var other => throw new InvalidOperationException("Unknown command " + other)
            };
            Console.WriteLine(reply!.ToJsonString());
        }
        return 0;
    }

    private static readonly Dictionary<BridgeCreationErrorKind, string> Kinds = new()
    {
        [BridgeCreationErrorKind.InvalidConfiguration] = "invalid_configuration",
        [BridgeCreationErrorKind.Claimed] = "claimed",
        [BridgeCreationErrorKind.System] = "system",
        [BridgeCreationErrorKind.LibraryUnavailable] = "library_unavailable",
        [BridgeCreationErrorKind.AbiMismatch] = "abi_mismatch",
    };

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
        catch (BridgeCreationException error)
        {
            worker.Dispose();
            return new JsonObject
            {
                ["error"] = new JsonObject { ["kind"] = Kinds[error.Kind], ["message"] = error.Message }
            };
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
        catch (ArgumentException)
        {
            return new JsonObject { ["rejected"] = "invalid_settings" };
        }
    }

    private static JsonNode Take()
    {
        if (!requests.TryTake(out var request, 10000)) throw new TimeoutException("No execution entered the host");
        return new JsonObject { ["request_id"] = request.RequestId };
    }

    private static JsonNode Reconnect()
    {
        core.Reconnect();
        return new JsonObject { ["reconnected"] = true };
    }

    private static JsonNode Close()
    {
        if (!core.Stop()) return new JsonObject { ["closed"] = false };
        core.Dispose();
        scheduler.Dispose();
        return new JsonObject { ["closed"] = true };
    }
}
