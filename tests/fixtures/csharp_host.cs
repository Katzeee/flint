using System;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Threading;
using Flint.Bridge;

internal static class Program
{
    private static int Main(string[] args)
    {
        try
        {
            var config = JsonSerializer.Serialize(new
            {
                host = "csharp",
                address = "127.0.0.1",
                port = int.Parse(args[1]),
                name = "Standalone C# runtime",
                runtime_version = Environment.Version.ToString(),
                enabled = true
            });
            using var bridge = new NativeBridge(args[0], config);
            var deadline = DateTime.UtcNow.AddSeconds(20);
            while ((!bridge.Connected || string.IsNullOrEmpty(bridge.InstanceId)) &&
                   DateTime.UtcNow < deadline)
                Thread.Sleep(25);
            if (!bridge.Connected || string.IsNullOrEmpty(bridge.InstanceId))
                throw new TimeoutException("The exported C# Bridge did not connect");
            File.WriteAllText(args[2], JsonSerializer.Serialize(new
            {
                pid = Process.GetCurrentProcess().Id,
                instance_id = bridge.InstanceId
            }));
            while (!File.Exists(args[3]))
            {
                var message = bridge.Poll(100);
                if (message == null) continue;
                using var request = JsonDocument.Parse(message);
                var id = request.RootElement.GetProperty("request_id").GetString();
                var code = request.RootElement.GetProperty("code").GetString();
                if (code == "ping")
                {
                    if (!bridge.Submit(JsonSerializer.Serialize(new
                    {
                        kind = "output",
                        request_id = id,
                        stdout = "CSHARP_ZIP_OK\n",
                        stderr = ""
                    }))) throw new InvalidOperationException("Output was rejected");
                    if (!bridge.Submit(JsonSerializer.Serialize(new
                    {
                        kind = "result",
                        request_id = id,
                        succeeded = true
                    }))) throw new InvalidOperationException("Result was rejected");
                }
                else
                {
                    if (!bridge.Submit(JsonSerializer.Serialize(new
                    {
                        kind = "result",
                        request_id = id,
                        succeeded = false,
                        error = "unsupported_test_command"
                    }))) throw new InvalidOperationException("Rejection was rejected");
                }
            }
            return 0;
        }
        catch (Exception exception)
        {
            Console.Error.WriteLine(exception);
            return 1;
        }
    }
}
