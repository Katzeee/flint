using System;
using System.Runtime.Serialization;
using Flint.Bridge;

namespace Flint.Unity
{
    /// <summary>The Unity adapter shared by Editor startup and external attach.</summary>
    public static class EditorBridge
    {
        private static readonly BridgeManager manager = new BridgeManager("unity", UnityRuntime.Version,
            () => new ExecutionCapabilities(new UnityExecution(), UnityRuntime.Callbacks), UnityRuntime.Callbacks.Post);
        private const string SessionKey = "Flint.Bridge.Connection";

        [DataContract]
        private sealed class SavedConnection
        {
            [DataMember] public string Library;
            [DataMember] public HostSettings Settings;
        }

        static EditorBridge()
        {
            ReflectionApi.Subscribe(UnityRuntime.Editor, null, "update", _ => UnityRuntime.Callbacks.Drain());
            ReflectionApi.Subscribe(ReflectionApi.Find("UnityEditor.AssemblyReloadEvents"), null,
                "beforeAssemblyReload", _ => manager.Dispose());
            ReflectionApi.Subscribe(UnityRuntime.Editor, null, "quitting", _ => manager.Dispose());
        }

        private static HostSettings Settings(string address, int port, string name)
        {
            return new HostSettings { Address = address, Port = port, Name = name };
        }

        public static bool Connected { get { return manager.Connected; } }
        public static bool Busy { get { return manager.Busy; } }
        public static string StatusJson { get { return manager.StatusJson; } }

        public static void Connect(string nativeLibrary, string address, int port, string name)
        {
            manager.Connect(nativeLibrary, Settings(address, port, name));
            SaveConnection();
        }

        public static void ApplySettings(string address, int port, string name)
        {
            manager.Configure(Settings(address, port, name));
            SaveConnection();
        }

        public static void Reconnect() { manager.Reconnect(); }
        public static bool Disconnect()
        {
            if (!manager.Disconnect()) return false;
            SaveConnection();
            return true;
        }

        // The package supplies defaults only on the first startup in this Editor session.
        public static void Start(string nativeLibrary, string address, int port, string name)
        {
            if (ReadConnection().Length == 0) Connect(nativeLibrary, address, port, name);
            else Restore();
        }

        internal static void Attach(string nativeLibrary, HostSettings settings)
        {
            manager.Attach(() =>
            {
                manager.Configure(settings, nativeLibrary);
                SaveConnection();
            });
        }

        internal static void Restore() { UnityRuntime.Callbacks.Post(RestoreOnUpdate); }

        private static void RestoreOnUpdate()
        {
            try
            {
                if ((bool)ReflectionApi.Get(UnityRuntime.Editor, "isCompiling") ||
                    (bool)ReflectionApi.Get(UnityRuntime.Editor, "isUpdating"))
                {
                    UnityRuntime.Callbacks.Post(RestoreOnUpdate);
                    return;
                }
                if (manager.StatusJson != null) return;
                var json = ReadConnection();
                if (json.Length == 0) return;
                var saved = Json.Read<SavedConnection>(json);
                if (saved.Settings == null) return;
                Connect(saved.Library, saved.Settings.Address, saved.Settings.Port, saved.Settings.Name);
            }
            catch (Exception error)
            {
                ReflectionApi.Find("UnityEngine.Debug").GetMethod("LogException", new[] { typeof(Exception) })
                    .Invoke(null, new object[] { error });
            }
        }

        private static string ReadConnection()
        {
            return (string)ReflectionApi.Find("UnityEditor.SessionState").GetMethod("GetString")
                .Invoke(null, new object[] { SessionKey, "" });
        }

        private static void SaveConnection()
        {
            var saved = new SavedConnection { Library = manager.NativeLibrary, Settings = manager.Settings };
            ReflectionApi.Find("UnityEditor.SessionState").GetMethod("SetString")
                .Invoke(null, new object[] { SessionKey, Json.Write(saved) });
        }
    }
}
