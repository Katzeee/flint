using System;
using UnityEditor;
using UnityEngine;
using UnityEngine.UIElements;

namespace Flint.Unity
{
    /// <summary>Per-user connection preferences and their Editor UI.</summary>
    internal static class EditorConnectionSettings
    {
        internal const string AddressKey = "Flint.Bridge.BridgeAddress";
        internal const string PortKey = "Flint.Bridge.BridgePort";
        internal const string NameKey = "Flint.Bridge.InstanceName";

        [Serializable]
        internal sealed class Values
        {
            public string address;
            public int port;
            public string name;
        }

        internal static Values Read()
        {
            return new Values
            {
                address = EditorPrefs.GetString(AddressKey, "127.0.0.1"),
                port = EditorPrefs.GetInt(PortKey, 6321),
                name = EditorPrefs.GetString(NameKey, "Unity Editor"),
            };
        }

        internal static void Save(Values values)
        {
            if (string.IsNullOrWhiteSpace(values.address) || string.IsNullOrWhiteSpace(values.name))
                throw new ArgumentException("Address and instance name are required");
            if (values.port < 1 || values.port > 65535)
                throw new ArgumentException("Port must be between 1 and 65535");
            EditorPrefs.SetString(AddressKey, values.address);
            EditorPrefs.SetInt(PortKey, values.port);
            EditorPrefs.SetString(NameKey, values.name);
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
            private sealed class Obstacle
            {
                public string kind;
                public string message;
            }

            [Serializable]
            private sealed class Connection
            {
                public string state;
                public Obstacle obstacle;
            }

            [Serializable]
            private sealed class Snapshot
            {
                public Connection connection;
                public bool busy;
                public Values settings;
            }

            private Values draft;
            private string actionError;
            private GUIStyle noteStyle;

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
                if (noteStyle == null) noteStyle = new GUIStyle(EditorStyles.label) { wordWrap = true };
                var labelWidth = EditorGUIUtility.labelWidth;
                EditorGUIUtility.labelWidth = 250;
                try
                {
                    using (new EditorGUILayout.HorizontalScope())
                    {
                        GUILayout.Space(10);
                        using (new EditorGUILayout.VerticalScope())
                        {
                            GUILayout.Space(10);
                            if (draft == null) draft = Read();
                            Snapshot snapshot = null;
                            var json = EditorBridge.StatusJson;
                            if (!string.IsNullOrEmpty(json)) snapshot = JsonUtility.FromJson<Snapshot>(json);

                            EditorGUILayout.LabelField("Connection", EditorStyles.boldLabel);
                            var state = snapshot?.connection?.state;
                            EditorGUILayout.LabelField("Status", string.IsNullOrEmpty(state) ? "Stopped" :
                                char.ToUpperInvariant(state[0]) + state.Substring(1).Replace('_', ' '));
                            EditorGUILayout.LabelField("Active settings", snapshot == null || snapshot.settings == null
                                ? "—" : snapshot.settings.address + ":" + snapshot.settings.port + " · " + snapshot.settings.name);
                            var saved = Read();
                            if (snapshot != null && (saved.address != snapshot.settings.address ||
                                saved.port != snapshot.settings.port || saved.name != snapshot.settings.name))
                                EditorGUILayout.LabelField("New settings will take effect on the next connection.", noteStyle);
                            // JsonUtility fills an absent obstacle with empty fields.
                            var obstacle = snapshot?.connection?.obstacle?.message;
                            if (!string.IsNullOrEmpty(obstacle))
                                EditorGUILayout.HelpBox(obstacle, MessageType.Warning);

                            EditorGUILayout.Space();
                            EditorGUILayout.LabelField("Settings", EditorStyles.boldLabel);
                            draft.address = EditorGUILayout.TextField("Bridge address", draft.address);
                            draft.port = EditorGUILayout.IntField("Bridge port", draft.port);
                            draft.name = EditorGUILayout.TextField("Instance name", draft.name);

                            EditorGUILayout.Space();
                            using (new EditorGUILayout.HorizontalScope())
                            {
                                if (GUILayout.Button("Apply", GUILayout.Height(26))) Apply();
                                using (new EditorGUI.DisabledScope(snapshot != null && snapshot.busy))
                                {
                                    if (GUILayout.Button(snapshot == null ? "Connect" : "Disconnect", GUILayout.Height(26)))
                                        ToggleConnection();
                                }
                            }
                            // A refused action's result, kept until the next action.
                            if (!string.IsNullOrEmpty(actionError))
                                EditorGUILayout.HelpBox(actionError, MessageType.Error);
                        }
                        GUILayout.Space(10);
                    }
                }
                finally { EditorGUIUtility.labelWidth = labelWidth; }
            }

            private void Apply()
            {
                try
                {
                    Save(draft);
                    actionError = null;
                }
                catch (Exception error) { actionError = error.Message; }
            }

            private void ToggleConnection()
            {
                try
                {
                    if (EditorBridge.StatusJson == null) EditorBootstrap.Connect();
                    else if (!EditorBridge.Disconnect()) throw new InvalidOperationException("Bridge is still executing host code");
                    actionError = null;
                }
                catch (Exception error) { actionError = error.Message; }
            }
        }
    }
}
