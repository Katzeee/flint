using System.IO;
using UnityEditor;

namespace Flint.Unity
{
    [InitializeOnLoad]
    internal static class EditorBootstrap
    {
        static EditorBootstrap()
        {
            EditorApplication.delayCall += Connect;
        }

        private static void Connect()
        {
            var package = UnityEditor.PackageManager.PackageInfo.FindForPackageName("com.flint.bridge");
            if (package == null) return;

            var library = Path.Combine(package.resolvedPath, "Editor", "Plugins", "flint_bridge_core.dll");

            var settings = EditorConnectionSettings.Read();
            EditorBridge.Connect(library, settings.address, settings.port, settings.name, settings.enabled);
        }
    }
}
