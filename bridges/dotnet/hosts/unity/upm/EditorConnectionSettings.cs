using System;
using UnityEditor;
using UnityEngine;
using UnityEngine.UIElements;

namespace Flint.Unity
{
    /// <summary>Per-user connection preferences and their Editor UI.</summary>
    internal static class EditorConnectionSettings
    {
        internal const string AddressKey = "Flint.Bridge.RegistryAddress";
        internal const string PortKey = "Flint.Bridge.RegistryPort";
        internal const string NameKey = "Flint.Bridge.InstanceName";
        internal const string EnabledKey = "Flint.Bridge.Enabled";

        [Serializable]
        internal sealed class Values
        {
            public string address;
            public int port;
            public string name;
            public bool enabled;
        }

        internal static Values Read()
        {
            return new Values
            {
                address = EditorPrefs.GetString(AddressKey, "127.0.0.1"),
                port = EditorPrefs.GetInt(PortKey, 6321),
                name = EditorPrefs.GetString(NameKey, "Unity Editor"),
                enabled = EditorPrefs.GetBool(EnabledKey, true)
            };
        }

        internal static void Save(Values values)
        {
            EditorPrefs.SetString(AddressKey, values.address);
            EditorPrefs.SetInt(PortKey, values.port);
            EditorPrefs.SetString(NameKey, values.name);
            EditorPrefs.SetBool(EnabledKey, values.enabled);
        }

        [MenuItem("Window/Flint Bridge/Connection Settings")]
        private static void Open()
        {
            SettingsService.OpenUserPreferences("Preferences/Flint Bridge");
        }

        [SettingsProvider]
        private static SettingsProvider CreateProvider()
        {
            return new ConnectionProvider();
        }

        private sealed class ConnectionProvider : SettingsProvider
        {
            [Serializable]
            private sealed class Snapshot
            {
                public string connection;
                public string last_error;
                public bool busy;
                public Values settings;
            }

            private Values draft;
            private string actionError;

            internal ConnectionProvider() : base("Preferences/Flint Bridge", SettingsScope.User)
            {
                inspectorUpdateHandler += Repaint;
            }

            public override void OnActivate(string searchContext, VisualElement rootElement)
            {
                draft = Read();
            }

            public override void OnGUI(string searchContext)
            {
                if (draft == null) draft = Read();
                Snapshot snapshot = null;
                var json = EditorBridge.StatusJson;
                if (!string.IsNullOrEmpty(json)) snapshot = JsonUtility.FromJson<Snapshot>(json);

                EditorGUILayout.LabelField("Connection", EditorStyles.boldLabel);
                EditorGUILayout.LabelField("Status", snapshot == null ? "Stopped" :
                    char.ToUpperInvariant(snapshot.connection[0]) + snapshot.connection.Substring(1).Replace('_', ' '));
                EditorGUILayout.LabelField("Active settings", snapshot == null || snapshot.settings == null
                    ? "—" : snapshot.settings.address + ":" + snapshot.settings.port + " · " + snapshot.settings.name);
                var error = !string.IsNullOrEmpty(actionError) ? actionError : snapshot?.last_error;
                if (!string.IsNullOrEmpty(error))
                    EditorGUILayout.HelpBox(error, MessageType.Warning);

                EditorGUILayout.Space();
                EditorGUILayout.LabelField("Settings", EditorStyles.boldLabel);
                draft.address = EditorGUILayout.TextField("Registry address", draft.address);
                draft.port = EditorGUILayout.IntField("Registry port", draft.port);
                draft.name = EditorGUILayout.TextField("Instance name", draft.name);
                draft.enabled = EditorGUILayout.Toggle("Connect to Flint", draft.enabled);

                EditorGUILayout.Space();
                var buttons = GUILayoutUtility.GetRect(0, 26, GUILayout.ExpandWidth(true));
                const float gap = 8;
                var width = (buttons.width - gap) / 2;
                using (new EditorGUI.DisabledScope(snapshot == null || snapshot.busy))
                {
                    if (GUI.Button(new Rect(buttons.x, buttons.y, width, buttons.height), "Apply")) Apply();
                }
                using (new EditorGUI.DisabledScope(snapshot == null || snapshot.busy ||
                    snapshot.settings == null || !snapshot.settings.enabled))
                {
                    if (GUI.Button(new Rect(buttons.x + width + gap, buttons.y, width, buttons.height), "Reconnect")) Retry();
                }
            }

            private void Apply()
            {
                try
                {
                    EditorBridge.ApplySettings(draft.address, draft.port, draft.name, draft.enabled);
                    Save(draft);
                    actionError = null;
                }
                catch (Exception error) { actionError = error.Message; }
            }

            private void Retry()
            {
                try
                {
                    EditorBridge.Reconnect();
                    actionError = null;
                }
                catch (Exception error) { actionError = error.Message; }
            }
        }
    }
}
