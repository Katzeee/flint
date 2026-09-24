using System;
using System.ComponentModel;
using System.IO;
using System.Net;
using System.Net.Sockets;
using Xunit;

namespace Flint.Bridge.Tests
{
    public class NativeBridgeTests
    {
        private static string Core
        {
            get
            {
                var path = Environment.GetEnvironmentVariable("FLINT_BRIDGE_CORE");
                if (string.IsNullOrEmpty(path) || !File.Exists(path))
                    throw new InvalidOperationException(
                        "FLINT_BRIDGE_CORE must point to the native core built by `cargo build --locked -p flint-bridge-core`; `cargo xtask test csharp` sets it");
                return path;
            }
        }

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

        [Fact]
        public void RejectsNullArguments()
        {
            Assert.Throws<ArgumentNullException>(() => new NativeBridge(null, "{}"));
            Assert.Throws<ArgumentNullException>(() => new NativeBridge(Core, null));
            using (var bridge = new NativeBridge(Core, Config()))
                Assert.Throws<ArgumentNullException>(() => bridge.Submit(null));
        }

        [Fact]
        public void MissingLibraryReportsItsPath()
        {
            var missing = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"), "flint_bridge_core.dll");
            var error = Assert.Throws<Win32Exception>(() => new NativeBridge(missing, Config()));
            Assert.Contains(missing, error.Message);
        }

        [Fact]
        public void RejectedConfigurationThrows()
        {
            Assert.Throws<ArgumentException>(() => new NativeBridge(Core, "{}"));
        }

        [Fact]
        public void NonAsciiConfigurationIsMarshaledAsUtf8()
        {
            using (new NativeBridge(Core, Config("场景 🌍"))) { }
        }

        [Fact]
        public void UnconnectedBridgeMarshalsIdleState()
        {
            using (var bridge = new NativeBridge(Core, Config()))
            {
                Assert.False(bridge.Connected);
                Assert.False(bridge.Busy);
                Assert.Equal("", bridge.InstanceId);
                Assert.Null(bridge.Poll(0));
            }
        }

        [Fact]
        public void DisposeIsIdempotentAndRejectsFurtherUse()
        {
            var bridge = new NativeBridge(Core, Config());
            bridge.Dispose();
            bridge.Dispose();
            Assert.Throws<ObjectDisposedException>(() => bridge.Connected);
            Assert.Throws<ObjectDisposedException>(() => bridge.Poll(0));
        }
    }
}
