using System.IO;
using Flint.Bridge;
using UnityEditor;
using UnityEngine;

namespace Flint.Unity
{
    [InitializeOnLoad]
    internal static class EditorBootstrap
    {
        static EditorBootstrap()
        {
            EditorApplication.delayCall += () =>
            {
                try { Start(); }
                catch (BridgeCreationException error)
                {
                    // The settings page stays available so the user can start it with Connect.
                    Debug.LogWarning("Flint Bridge did not start: " + error.Message);
                }
            };
        }

        internal static void Connect()
        {
            var library = NativeLibrary();
            if (library == null) return;
            var settings = EditorConnectionSettings.Read();
            EditorBridge.Connect(library, settings.address, settings.port, settings.name);
        }

        private static void Start()
        {
            var library = NativeLibrary();
            if (library == null) return;
            var settings = EditorConnectionSettings.Read();
            EditorBridge.Start(library, settings.address, settings.port, settings.name);
        }

        private static string NativeLibrary()
        {
            var package = UnityEditor.PackageManager.PackageInfo.FindForPackageName("com.flint.bridge");
            return package == null ? null : Path.Combine(package.resolvedPath, "Editor", "Plugins", "flint_bridge_core.dll");
        }
    }
}
