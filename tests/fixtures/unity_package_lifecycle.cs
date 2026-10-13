using System;
using System.IO;
using UnityEditor;

namespace Flint.Unity
{
    [InitializeOnLoad]
    internal static class FlintTestLifecycle
    {
        private const string PhaseKey = "Flint.Test.Lifecycle";
        private static int phase = SessionState.GetInt(PhaseKey, 0);
        private static int refreshes;

        static FlintTestLifecycle()
        {
            if (phase != 4) EditorApplication.update += Check;
        }

        private static void Check()
        {
            try
            {
                if (phase == 0)
                {
                    if (!EditorBridge.Connected) return;
                    SessionState.SetInt(PhaseKey, 1);
                    EditorUtility.RequestScriptReload();
                    phase = -1;
                    return;
                }
                if (phase == -1) return;
                if (phase == 1)
                {
                    if (!EditorBridge.Connected) return;
                    if (!EditorBridge.Disconnect()) throw new Exception("Idle Bridge refused disconnect");
                    SessionState.SetInt(PhaseKey, 2);
                    EditorUtility.RequestScriptReload();
                    phase = -1;
                    return;
                }
                if (phase == 2)
                {
                    if (EditorBridge.StatusJson != null) throw new Exception("Bridge restarted without Connect");
                    if (++refreshes < 30) return;
                    EditorBootstrap.Connect();
                    phase = 3;
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
            SessionState.SetInt(PhaseKey, 4);
            var path = Environment.GetEnvironmentVariable("FLINT_TEST_LIFECYCLE_RESULT");
            File.WriteAllText(path + ".tmp", result);
            File.Move(path + ".tmp", path);
        }
    }

    [InitializeOnLoad]
    internal static class FlintTestReload
    {
        private static readonly string Trigger = Environment.GetEnvironmentVariable("FLINT_TEST_RELOAD_TRIGGER");

        static FlintTestReload()
        {
            File.AppendAllText(Trigger + ".loads", "loaded\n");
            EditorApplication.update += () =>
            {
                if (!File.Exists(Trigger)) return;
                File.Delete(Trigger);
                EditorUtility.RequestScriptReload();
            };
        }
    }
}
