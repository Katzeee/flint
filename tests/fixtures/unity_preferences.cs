using System;
using System.Collections.Generic;

namespace Flint.Unity
{
    // Compiled only into the disposable package under test. Unqualified EditorPrefs
    // calls use this store, leaving the user's Unity preferences untouched.
    internal static class EditorPrefs
    {
        private static readonly Dictionary<string, object> values = new Dictionary<string, object>
        {
            { "Flint.Bridge.BridgeAddress", "127.0.0.1" },
            { "Flint.Bridge.BridgePort", int.Parse(Environment.GetEnvironmentVariable("FLINT_TEST_BRIDGE_PORT")) },
            { "Flint.Bridge.InstanceName", "Unity Editor" },
        };

        public static string GetString(string key, string fallback) => values.TryGetValue(key, out var value) ? (string)value : fallback;
        public static int GetInt(string key, int fallback) => values.TryGetValue(key, out var value) ? (int)value : fallback;
        public static void SetString(string key, string value) => values[key] = value;
        public static void SetInt(string key, int value) => values[key] = value;
    }
}
