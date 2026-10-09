using System;
using Flint.Bridge;

namespace Flint.Unity
{
    /// <summary>The Unity adapter shared by Editor startup and external attach.</summary>
    public static class EditorBridge
    {
        private static readonly BridgeManager manager = new BridgeManager("unity", UnityRuntime.Version,
            () => new ExecutionCapabilities(new UnityExecution(), UnityRuntime.Callbacks), UnityRuntime.Callbacks.Post);

        static EditorBridge()
        {
            ReflectionApi.Subscribe(UnityRuntime.Editor, null, "update", _ => UnityRuntime.Callbacks.Drain());
            ReflectionApi.Subscribe(ReflectionApi.Find("UnityEditor.AssemblyReloadEvents"), null,
                "beforeAssemblyReload", _ => manager.Dispose());
            ReflectionApi.Subscribe(UnityRuntime.Editor, null, "quitting", _ => manager.Dispose());
        }

        private static BridgeSettings Settings(string address, int port, string name, bool enabled)
        {
            return new BridgeSettings { Address = address, Port = port, Name = name, Enabled = enabled };
        }

        public static BridgeManager Manager { get { return manager; } }
        public static bool Connected { get { return manager.Connected; } }
        public static bool Busy { get { return manager.Busy; } }
        public static string StatusJson { get { return manager.StatusJson; } }

        public static void Connect(string nativeLibrary, string address, int port, string name, bool enabled = true)
        {
            manager.Connect(nativeLibrary, Settings(address, port, name, enabled));
        }

        public static void ApplySettings(string address, int port, string name, bool enabled)
        {
            manager.Configure(Settings(address, port, name, enabled));
        }

        public static void Reconnect() { manager.Reconnect(); }
        public static bool Disconnect() { return manager.Disconnect(); }
    }
}
