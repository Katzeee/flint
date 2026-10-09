using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Flint.Bridge;

namespace Flint.Unity
{
    internal sealed class UnityExecution : IExecutor
    {
        private readonly AsyncLocal<object> logScope = new AsyncLocal<object>();

        /// <summary>A built assembly and the temporary files the Bridge disposes after use.</summary>
        private sealed class Program : IDisposable
        {
            internal string Folder, Assembly, ClassName;
            internal Action Unsubscribe;
            public void Dispose()
            {
                Unsubscribe?.Invoke();
                if (Directory.Exists(Folder)) Directory.Delete(Folder, true);
            }
        }

        public Task<object> Prepare(ExecutionRequest request)
        {
            var completion = new TaskCompletionSource<object>();
            Build(request, completion);
            return completion.Task;
        }

        // The compiler may be busy; retry from the Editor loop until a build starts.
        private void Build(ExecutionRequest request, TaskCompletionSource<object> completion)
        {
            if ((bool)ReflectionApi.Get(UnityRuntime.Editor, "isCompiling"))
            {
                UnityRuntime.Callbacks.Post(() => Build(request, completion));
                return;
            }
            var program = new Program
            {
                Folder = Path.Combine(Path.GetDirectoryName((string)ReflectionApi.Get(UnityRuntime.Application, "dataPath")),
                    "Temp", "Flint", Guid.NewGuid().ToString("N")),
                ClassName = "FlintExecution_" + Guid.NewGuid().ToString("N"),
            };
            try
            {
                var source = CsharpSource.WriteMethod(program.Folder, program.ClassName, request.Code ?? "",
                    "using System; using UnityEngine; using UnityEditor;");
                var builder = Activator.CreateInstance(UnityRuntime.Builder,
                    Path.Combine(program.Folder, program.ClassName + ".dll"), new[] { source });
                program.Unsubscribe = ReflectionApi.Subscribe(UnityRuntime.Builder, builder, "buildFinished", arguments =>
                {
                    var errors = new StringBuilder();
                    foreach (var message in (Array)arguments[1])
                        if (ReflectionApi.Member(message, "type").ToString() == "Error")
                            errors.AppendLine(ReflectionApi.Member(message, "file") + ":" + ReflectionApi.Member(message, "line") +
                                ": " + ReflectionApi.Member(message, "message"));
                    if (errors.Length != 0)
                    {
                        program.Dispose();
                        completion.TrySetException(new InvalidOperationException(errors.ToString()));
                        return;
                    }
                    program.Assembly = (string)arguments[0];
                    completion.TrySetResult(program);
                });
                if (!(bool)UnityRuntime.Builder.GetMethod("Build").Invoke(builder, null))
                {
                    program.Dispose();
                    UnityRuntime.Callbacks.Post(() => Build(request, completion));
                }
            }
            catch (Exception error)
            {
                program.Dispose();
                completion.TrySetException(error);
            }
        }

        public Task Run(object preparedResult, TextWriter stdout, TextWriter stderr)
        {
            var program = (Program)preparedResult;
            var scope = new object();
            var unsubscribe = ReflectionApi.Subscribe(UnityRuntime.Application, null, "logMessageReceivedThreaded", arguments =>
            {
                if (logScope.Value != scope) return;
                var type = arguments[2].ToString();
                (type == "Error" || type == "Exception" || type == "Assert" ? stderr : stdout).WriteLine((string)arguments[0]);
            });
            try { logScope.Value = scope; ManagedAssembly.Invoke(program.Assembly, program.ClassName); }
            finally { logScope.Value = null; unsubscribe(); }
            return Task.CompletedTask;
        }
    }
}
