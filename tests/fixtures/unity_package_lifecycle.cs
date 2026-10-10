using System;
using System.IO;
using UnityEditor;

namespace Flint.Unity
{
    [InitializeOnLoad]
    internal static class FlintTestLifecycle
    {
        private static int phase;
        private static int refreshes;

        static FlintTestLifecycle() { EditorApplication.update += Check; }

        private static void Check()
        {
            try
            {
                if (phase == 0)
                {
                    if (!EditorBridge.Connected) return;
                    if (!EditorBridge.Disconnect()) throw new Exception("Idle Bridge refused disconnect");
                    phase = 1;
                    return;
                }
                if (phase == 1)
                {
                    if (EditorBridge.StatusJson != null) throw new Exception("Bridge restarted without Connect");
                    if (++refreshes < 3) return;
                    var package = UnityEditor.PackageManager.PackageInfo.FindForPackageName("com.flint.bridge");
                    var settings = EditorConnectionSettings.Read();
                    EditorBridge.Connect(Path.Combine(package.resolvedPath, "Editor", "Plugins", "flint_bridge_core.dll"),
                        settings.address, settings.port, settings.name);
                    phase = 2;
                    return;
                }
                if (!EditorBridge.Connected) return;
                Finish("ok");
            }
            catch (Exception error) { Finish(error.ToString()); }
        }

        private static void Finish(string result)
        {
            EditorApplication.update -= Check;
            var path = Environment.GetEnvironmentVariable("FLINT_TEST_LIFECYCLE_RESULT");
            File.WriteAllText(path + ".tmp", result);
            File.Move(path + ".tmp", path);
        }
    }
}
