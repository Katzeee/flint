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
                try { Connect(); }
                catch (BridgeCreationException error)
                {
                    // The settings page stays available so the user can start it with Connect.
                    Debug.LogWarning("Flint Bridge did not start: " + error.Message);
                }
            };
        }

        internal static void Connect()
        {
            var package = UnityEditor.PackageManager.PackageInfo.FindForPackageName("com.flint.bridge");
            if (package == null) return;

            var library = Path.Combine(package.resolvedPath, "Editor", "Plugins", "flint_bridge_core.dll");

            var settings = EditorConnectionSettings.Read();
            EditorBridge.Connect(library, settings.address, settings.port, settings.name);
        }
    }
}
