using System;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Flint.Bridge;

internal static class Program
{
    private sealed class Command : IDisposable
    {
        internal string Code, Cleaned;
        public void Dispose() { if (Cleaned != null) File.WriteAllText(Cleaned, "cleaned"); }
    }
    private sealed class Executor : IExecutor
    {
        private readonly string release;
        internal Executor(string release) { this.release = release; }
        public async Task<object> Prepare(ExecutionRequest request)
        {
            await Task.Yield();
            return new Command { Code = request.Code, Cleaned = request.Code == "async" ? release + ".cleaned" : null };
        }
        public async Task Run(object prepared, TextWriter stdout, TextWriter stderr)
        {
            var code = ((Command)prepared).Code;
            if (code == "ping") { stdout.Write("CSHARP_ZIP_OK\n"); return; }
            if (code != "async") throw new InvalidOperationException("unsupported_test_command");
            stdout.Write("BEGIN\0🙂\n");
            GC.Collect();
            var deadline = DateTime.UtcNow.AddSeconds(20);
            while (!File.Exists(release))
            {
                if (DateTime.UtcNow >= deadline) throw new TimeoutException("Test did not release async execution");
                await Task.Delay(10);
            }
            stderr.Write("异步完成\n");
        }
    }
    private static int Main(string[] args)
    {
        try
        {
            var config = JsonSerializer.Serialize(new
            {
                host = "standalone_csharp",
                runtime_version = Environment.Version.ToString(),
                settings = new
                {
                    address = "127.0.0.1",
                    port = int.Parse(args[1]),
                    name = "Standalone C# runtime",
                }
            });
            using var bridge = new NativeCore(args[0], config,
                new ExecutionCapabilities(new Executor(args[3] + ".release"), new WorkerThread()));
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
            while (!File.Exists(args[3])) Thread.Sleep(50);
            return 0;
        }
        catch (Exception exception)
        {
            Console.Error.WriteLine(exception);
            return 1;
        }
    }
}
