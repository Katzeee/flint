using System;
using System.Globalization;
using System.Text;
using Flint.Bridge;

namespace Flint.Unity
{
    /// <summary>
    /// Entry point invoked by the injected attach bootstrap inside the host's
    /// managed runtime (Mono today, CoreCLR later). It starts the Bridge, which
    /// connects and registers on its own native thread; executing host code on
    /// the Editor main thread is added on top of this.
    ///
    /// The assembly stays free of Unity references so one portable IL build runs
    /// on any Unity, on Mono or CoreCLR; the few Unity APIs it needs are reached
    /// by reflection.
    /// </summary>
    public static class Attach
    {
        private static readonly object Gate = new object();
        // Holds the started Bridge until this domain unloads.
        private static NativeBridge _bridge;

        /// <summary>
        /// Start the Bridge from the injected runtime thread. The argument is
        /// newline-delimited: address, port, instance name, native core path.
        /// </summary>
        public static void Initialize(string configuration)
        {
            lock (Gate)
            {
                if (_bridge != null) return; // one Bridge per process
                var parts = (configuration ?? string.Empty).Split('\n');
                if (parts.Length < 4)
                    throw new ArgumentException("Incomplete attach configuration");
                var address = parts[0];
                var port = int.Parse(parts[1], CultureInfo.InvariantCulture);
                var name = parts[2];
                var corePath = parts[3];
                var config = "{\"host\":\"unity\",\"address\":" + JsonString(address) +
                    ",\"port\":" + port.ToString(CultureInfo.InvariantCulture) +
                    ",\"name\":" + JsonString(name) +
                    ",\"runtime_version\":" + JsonString(RuntimeVersion()) + "}";
                _bridge = new NativeBridge(corePath, config);
                // A script reload unloads this domain but not the native core;
                // without this the core keeps its registration and process claim.
                AppDomain.CurrentDomain.DomainUnload += Release;
            }
        }

        private static void Release(object sender, EventArgs arguments)
        {
            lock (Gate)
            {
                if (_bridge == null) return;
                _bridge.Dispose();
                _bridge = null;
            }
        }

        /// <summary>The runtime label reported to the backend, via reflection so
        /// this assembly needs no compile-time Unity reference.</summary>
        private static string RuntimeVersion()
        {
            var application = Type.GetType("UnityEngine.Application, UnityEngine.CoreModule") ??
                Type.GetType("UnityEngine.Application, UnityEngine");
            var version = application?.GetProperty("unityVersion")?.GetValue(null, null) as string;
            if (!string.IsNullOrEmpty(version)) return "Unity " + version;
            return ".NET " + Environment.Version;
        }

        private static string JsonString(string value)
        {
            var builder = new StringBuilder("\"");
            foreach (var character in value ?? string.Empty)
            {
                if (character == '"' || character == '\\') builder.Append('\\').Append(character);
                else if (character == '\n') builder.Append("\\n");
                else if (character == '\r') builder.Append("\\r");
                else if (character == '\t') builder.Append("\\t");
                else if (character < 0x20) builder.Append("\\u").Append(((int)character).ToString("x4"));
                else builder.Append(character);
            }
            return builder.Append('"').ToString();
        }
    }
}
