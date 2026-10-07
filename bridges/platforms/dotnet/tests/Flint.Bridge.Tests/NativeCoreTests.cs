using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text.Json;
using System.Threading.Tasks;
using Xunit;

namespace Flint.Bridge.Tests
{
    internal sealed class IdleExecutor : IExecutor
    {
        public Task<object> Prepare(ExecutionRequest request) => throw new InvalidOperationException("Unexpected preparation");
        public Task Run(object prepared, TextWriter stdout, TextWriter stderr) => throw new InvalidOperationException("Unexpected invocation");
    }

    [Collection("Native Bridge")]
    public class NativeCoreTests
    {
        private static int UnusedPort()
        {
            var listener = new TcpListener(IPAddress.Loopback, 0);
            listener.Start();
            var port = ((IPEndPoint)listener.LocalEndpoint).Port;
            listener.Stop();
            return port;
        }

        private static string Config(string name = "tests") =>
            "{\"host\":\"csharp\",\"address\":\"127.0.0.1\",\"port\":" + UnusedPort() +
            ",\"name\":\"" + name + "\",\"runtime_version\":\"test\"}";

        private static ExecutionCapabilities Capabilities() => new ExecutionCapabilities(new IdleExecutor(), new CallbackQueue());

        [Fact]
        public void RejectsNullArguments()
        {
            Assert.Throws<ArgumentNullException>(() => new NativeCore(null, "{}", Capabilities()));
            Assert.Throws<ArgumentNullException>(() => new NativeCore(NativeLibrary.Path, null, Capabilities()));
            Assert.Throws<ArgumentNullException>(() => new NativeCore(NativeLibrary.Path, Config(), null));
            using (var bridge = new NativeCore(NativeLibrary.Path, Config(), Capabilities()))
                Assert.Throws<ArgumentNullException>(() => bridge.ApplySettings(null));
        }

        [Fact]
        public void MarshalsUnicodeConfigurationAndSettings()
        {
            using (var bridge = new NativeCore(NativeLibrary.Path, Config("场景 🌍"), Capabilities()))
            {
                using (var status = JsonDocument.Parse(bridge.StatusJson))
                    Assert.Equal("场景 🌍", status.RootElement.GetProperty("settings").GetProperty("name").GetString());
                bridge.ApplySettings("{\"address\":\"127.0.0.1\",\"port\":6321,\"name\":\"新场景\",\"enabled\":false}");
                using (var status = JsonDocument.Parse(bridge.StatusJson))
                    Assert.Equal("新场景", status.RootElement.GetProperty("settings").GetProperty("name").GetString());
            }
        }

        [Fact]
        public void StopWithoutExecutionAllowsDestructionAndReleasesTheClaim()
        {
            var bridge = new NativeCore(NativeLibrary.Path, Config(), Capabilities());
            Assert.True(bridge.Stop());
            Assert.Throws<BridgeStoppedException>(() => bridge.CheckRunning());
            bridge.Dispose();
            bridge.Dispose();
            Assert.Throws<ObjectDisposedException>(() => bridge.Connected);
            using (var replacement = new NativeCore(NativeLibrary.Path, Config(), Capabilities()))
                Assert.False(replacement.Busy);
        }

        [Fact]
        public void FailedCreationReleasesTheSchedulerThroughTheManager()
        {
            var scheduler = new CountingScheduler();
            using (var manager = new BridgeManager("csharp", () => "test",
                () => new ExecutionCapabilities(new IdleExecutor(), scheduler), callback => callback()))
            {
                var missing = Path.Combine(Path.GetTempPath(), "missing-flint-core.dll");
                var error = Assert.Throws<BridgeCreationException>(() =>
                    manager.Connect(missing, new BridgeSettings { Address = "127.0.0.1", Port = UnusedPort(), Name = "tests" }));
                Assert.Equal(BridgeCreationErrorKind.LibraryUnavailable, error.Kind);
            }
            Assert.Equal(1, scheduler.Disposed);
        }

        private sealed class CountingScheduler : IExecutionScheduler
        {
            internal int Disposed;
            public void Post(Action callback) => callback();
            public void Dispose() => Disposed++;
        }
    }
}
