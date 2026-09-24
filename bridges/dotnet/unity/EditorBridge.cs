using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text;
using System.Threading;
using Flint.Bridge;
using UnityEditor;
using UnityEditor.Compilation;
using UnityEngine;

namespace Flint.Unity
{
    /// <summary>Unity Editor main-thread adapter for the shared Flint connection core.</summary>
    [InitializeOnLoad]
    public static class EditorBridge
    {
        [Serializable]
        private sealed class Config
        {
            public string host = "unity";
            public string address;
            public int port;
            public string name;
            public string runtime_version;
        }

        [Serializable]
        private sealed class ExecuteEvent
        {
            public string request_id;
            public string code;
        }

        [Serializable]
        private sealed class Command
        {
            public string kind;
            public string request_id;
            public string stdout = "";
            public string stderr = "";
            public bool succeeded;
            public string traceback;
            public string error;
        }

        private static readonly Queue<ExecuteEvent> Pending = new Queue<ExecuteEvent>();
        private static readonly object PendingLock = new object();
        private static readonly AsyncLocal<string> LogScope = new AsyncLocal<string>();
        private static NativeBridge bridge;
        private static Thread poller;
        private static bool stopping;
        private static bool compiling;

        static EditorBridge()
        {
            EditorApplication.update += Update;
            AssemblyReloadEvents.beforeAssemblyReload += Disconnect;
            EditorApplication.quitting += Disconnect;
            var library = Environment.GetEnvironmentVariable("FLINT_UNITY_CORE");
            var portText = Environment.GetEnvironmentVariable("FLINT_UNITY_PORT");
            int port;
            if (!string.IsNullOrEmpty(library) && int.TryParse(portText, out port))
                EditorApplication.delayCall += () => Connect(library, "127.0.0.1", port, "Unity Editor");
        }

        public static bool Connected { get { return bridge != null && bridge.Connected; } }

        public static void Connect(string nativeLibrary, string address, int port, string name)
        {
            Disconnect();
            if (port < 1 || port > ushort.MaxValue) throw new ArgumentOutOfRangeException(nameof(port));
            var config = new Config
            {
                address = address,
                port = port,
                name = name,
                runtime_version = Application.unityVersion +
                    (Type.GetType("Mono.Runtime") != null ? " Mono" : " .NET")
            };
            var libraryPath = Path.GetFullPath(nativeLibrary);
            if (!File.Exists(libraryPath))
                throw new FileNotFoundException("Bridge core is missing", libraryPath);
            bridge = new NativeBridge(libraryPath, JsonUtility.ToJson(config));
            stopping = false;
            poller = new Thread(Poll) { IsBackground = true, Name = "flint-unity-poll" };
            poller.Start();
        }

        public static void Disconnect()
        {
            stopping = true;
            if (poller != null)
            {
                poller.Join();
                poller = null;
            }
            if (bridge != null)
            {
                bridge.Dispose();
                bridge = null;
            }
            lock (PendingLock) Pending.Clear();
        }

        private static void Poll()
        {
            while (!stopping)
            {
                var message = bridge.Poll(100);
                if (message == null) continue;
                var request = JsonUtility.FromJson<ExecuteEvent>(message);
                if (request != null && !string.IsNullOrEmpty(request.request_id))
                    lock (PendingLock) Pending.Enqueue(request);
            }
        }

        private static void Update()
        {
            if (bridge == null || compiling || EditorApplication.isCompiling) return;
            ExecuteEvent request;
            lock (PendingLock)
            {
                if (Pending.Count == 0) return;
                request = Pending.Dequeue();
            }
            try { Compile(request); }
            catch (Exception exception)
            {
                compiling = false;
                Finish(request, false, exception.ToString(), "compile_error");
            }
        }

        private static void Compile(ExecuteEvent request)
        {
            var directory = Path.Combine(Path.GetDirectoryName(Application.dataPath), "Temp", "Flint");
            Directory.CreateDirectory(directory);
            var className = "FlintExecution_" + Guid.NewGuid().ToString("N");
            var source = Path.Combine(directory, className + ".cs");
            var assembly = Path.Combine(directory, className + ".dll");
            File.WriteAllText(source,
                "using System; using UnityEngine; using UnityEditor;\n" +
                "public static class " + className + " { public static void Run() {\n" +
                (request.code ?? "") + "\n} }\n", Encoding.UTF8);
            var builder = new AssemblyBuilder(assembly, new[] { source });
            builder.buildFinished += (path, messages) =>
            {
                compiling = false;
                try
                {
                    var errors = new StringBuilder();
                    foreach (var message in messages)
                        if (message.type == CompilerMessageType.Error)
                            errors.AppendLine(message.file + ":" + message.line + ": " + message.message);
                    if (errors.Length != 0)
                        Finish(request, false, errors.ToString(), "compile_error");
                    else
                        Run(request, path, className);
                }
                catch (Exception exception)
                {
                    Finish(request, false, exception.ToString(), "execution_error");
                }
                finally
                {
                    File.Delete(source);
                    File.Delete(assembly);
                    File.Delete(Path.ChangeExtension(assembly, ".pdb"));
                }
            };
            compiling = true;
            if (!builder.Build())
            {
                compiling = false;
                lock (PendingLock) Pending.Enqueue(request);
                File.Delete(source);
            }
        }

        private static void Run(ExecuteEvent request, string path, string className)
        {
            var stdout = new StringBuilder();
            var stderr = new StringBuilder();
            var outputLock = new object();
            Application.LogCallback callback = (message, stack, type) =>
            {
                if (LogScope.Value != request.request_id) return;
                lock (outputLock)
                {
                    var target = type == LogType.Error || type == LogType.Exception || type == LogType.Assert
                        ? stderr : stdout;
                    target.AppendLine(message);
                }
            };
            Application.logMessageReceivedThreaded += callback;
            try
            {
                LogScope.Value = request.request_id;
                var assembly = System.Reflection.Assembly.Load(File.ReadAllBytes(path));
                assembly.GetType(className, true).GetMethod("Run").Invoke(null, null);
                Submit(new Command { kind = "output", request_id = request.request_id,
                    stdout = stdout.ToString(), stderr = stderr.ToString() });
                Finish(request, true, null, null);
            }
            catch (Exception exception)
            {
                var actual = exception is TargetInvocationException && exception.InnerException != null
                    ? exception.InnerException : exception;
                Submit(new Command { kind = "output", request_id = request.request_id,
                    stdout = stdout.ToString(), stderr = stderr.ToString() });
                Finish(request, false, actual.ToString(), "execution_error");
            }
            finally
            {
                LogScope.Value = null;
                Application.logMessageReceivedThreaded -= callback;
            }
        }

        private static void Finish(ExecuteEvent request, bool succeeded, string traceback, string error)
        {
            Submit(new Command { kind = "result", request_id = request.request_id,
                succeeded = succeeded, traceback = traceback, error = error });
        }

        private static void Submit(Command command)
        {
            if (bridge != null) bridge.Submit(JsonUtility.ToJson(command));
        }
    }
}
