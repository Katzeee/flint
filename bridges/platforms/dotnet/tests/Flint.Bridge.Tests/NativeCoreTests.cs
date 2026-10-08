using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Threading.Tasks;
using Xunit;

namespace Flint.Bridge.Tests
{
    internal sealed class IdleExecutor : IExecutor
    {
        public Task<object> Prepare(ExecutionRequest request) => throw new InvalidOperationException("Unexpected preparation");
        public Task Run(object preparedResult, TextWriter stdout, TextWriter stderr) => throw new InvalidOperationException("Unexpected invocation");
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

        private static string Config() =>
            "{\"host\":\"csharp\",\"address\":\"127.0.0.1\",\"port\":" + UnusedPort() +
            ",\"name\":\"tests\",\"runtime_version\":\"test\"}";

        private static ExecutionCapabilities Capabilities() => new ExecutionCapabilities(new IdleExecutor(), new CallbackQueue());

        [Fact]
        public void StopWithoutExecutionAllowsDestructionAndReleasesTheClaim()
        {
            var bridge = new NativeCore(NativeLibrary.Path, Config(), Capabilities());
            try
            {
                Assert.True(bridge.Stop());
                Assert.Throws<BridgeStoppedException>(() => bridge.CheckRunning());
                bridge.Dispose();
                bridge.Dispose();
                Assert.Throws<ObjectDisposedException>(() => bridge.Connected);
            }
            finally { bridge.Dispose(); }
            using (var replacement = new NativeCore(NativeLibrary.Path, Config(), Capabilities()))
                Assert.False(replacement.Busy);
        }

    }
}
