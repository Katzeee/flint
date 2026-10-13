using System;
using System.Reflection;
using Flint.Bridge;

namespace Flint.Unity
{
    /// <summary>The attach bootstrap's entry into the Unity adapter.</summary>
    public static class EditorAttach
    {
        public static void Attach(string nativeLibrary, HostSettings settings)
        {
            EditorBridge.Attach(nativeLibrary, settings);
        }

        // The bootstrap enters a reloaded domain while the Editor is still loading
        // it, with only UnityEditor.CoreModule guaranteed. EditorBridge's static
        // state needs more, so it must first initialize on the main thread.
        public static void Restore()
        {
            // CallDelayed registers on the tick event and signals it thread-safely.
            // delayCall is a delegate field the main thread reads and clears, so
            // subscribing to it from this worker can lose the callback.
            var editor = ReflectionApi.Find("UnityEditor.EditorApplication");
            var schedule = editor.GetMethod("CallDelayed", BindingFlags.Static | BindingFlags.NonPublic);
            Action restore = () => EditorBridge.Restore();
            var callback = Delegate.CreateDelegate(schedule.GetParameters()[0].ParameterType, restore.Target, restore.Method);
            schedule.Invoke(null, new object[] { callback, 0d });
        }
    }
}
