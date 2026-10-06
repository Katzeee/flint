using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text.Json;
using Xunit;

namespace Flint.Bridge.Tests
{
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

        [Fact]
        public void RejectsNullArguments()
        {
            Assert.Throws<ArgumentNullException>(() => new NativeCore(null, "{}"));
            Assert.Throws<ArgumentNullException>(() => new NativeCore(NativeLibrary.Path, null));
            using (var bridge = new NativeCore(NativeLibrary.Path, Config()))
                Assert.Throws<ArgumentNullException>(() => bridge.ReportExecution(null));
        }

        [Fact]
        public void MarshalsUnicodeConfigurationAndSettings()
        {
            using (var bridge = new NativeCore(NativeLibrary.Path, Config("场景 🌍")))
            {
                using (var status = JsonDocument.Parse(bridge.StatusJson))
                    Assert.Equal("场景 🌍", status.RootElement.GetProperty("settings").GetProperty("name").GetString());
                bridge.ApplySettings("{\"address\":\"127.0.0.1\",\"port\":6321,\"name\":\"新场景\",\"enabled\":false}");
                using (var status = JsonDocument.Parse(bridge.StatusJson))
                {
                    Assert.Equal("新场景", status.RootElement.GetProperty("settings").GetProperty("name").GetString());
                }
            }
        }

        [Fact]
        public void DisposeIsIdempotentAndRejectsFurtherUse()
        {
            var bridge = new NativeCore(NativeLibrary.Path, Config());
            bridge.Dispose();
            bridge.Dispose();
            Assert.Throws<ObjectDisposedException>(() => bridge.Connected);
            Assert.Throws<ObjectDisposedException>(() => bridge.Poll(0));
        }
    }
}
