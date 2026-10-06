using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Nodes;
using Flint.Bridge;

// Drive the exported C# binding for the runtime conformance scenarios. Reads
// one JSON command per line and answers each with one JSON line. Results are
// reported as the binding produced them; Rust owns every assertion.
internal static class Program
{
    private static NativeCore core;
    private static string held;

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
                "finish" => new JsonObject
                {
                    ["reported"] = core.ReportExecution(JsonSerializer.Serialize(new
                    {
                        kind = "result",
                        request_id = held,
                        succeeded = true
                    }))
                },
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
        try
        {
            core = new NativeCore(library, config);
            return new JsonObject { ["created"] = true };
        }
        catch (BridgeCreationException error)
        {
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
        var deadline = DateTime.UtcNow.AddSeconds(10);
        while (DateTime.UtcNow < deadline)
        {
            var message = core.Poll(100);
            if (message == null) continue;
            held = JsonNode.Parse(message)!["request_id"]!.GetValue<string>();
            return new JsonObject { ["request_id"] = held };
        }
        return new JsonObject { ["request_id"] = null };
    }

    private static JsonNode Reconnect()
    {
        core.Reconnect();
        return new JsonObject { ["reconnected"] = true };
    }

    private static JsonNode Close()
    {
        core.Dispose();
        return new JsonObject { ["closed"] = true };
    }
}
