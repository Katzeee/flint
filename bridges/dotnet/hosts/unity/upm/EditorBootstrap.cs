using System.IO;
using UnityEditor;

namespace Flint.Unity
{
    [InitializeOnLoad]
    internal static class EditorBootstrap
    {
        private const int RegistryPort = 6321;

        static EditorBootstrap()
        {
            EditorApplication.delayCall += Connect;
        }

        private static void Connect()
        {
            var package = UnityEditor.PackageManager.PackageInfo.FindForPackageName("com.flint.bridge");
            if (package == null) return;

            var library = Path.Combine(package.resolvedPath, "Editor", "Plugins", "flint_bridge_core.dll");

            EditorBridge.Connect(library, "127.0.0.1", RegistryPort, "Unity Editor");
        }
    }
}
