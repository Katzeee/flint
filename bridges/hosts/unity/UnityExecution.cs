using System;
using System.IO;
using System.Text;
using System.Threading;
using Flint.Bridge;

namespace Flint.Unity
{
    internal sealed class UnityExecution : IExecutionAdapter
    {
        private readonly AsyncLocal<string> logScope = new AsyncLocal<string>();
        private bool disposed;

        public void Execute(ExecutionRequest request, Action<ExecutionReport> report)
        {
            if (disposed) return;
            if ((bool)ReflectionApi.Get(UnityRuntime.Editor, "isCompiling"))
            {
                UnityRuntime.Callbacks.Post(() => Execute(request, report));
                return;
            }
            var directory = Path.Combine(Path.GetDirectoryName((string)ReflectionApi.Get(UnityRuntime.Application, "dataPath")), "Temp", "Flint");
            var className = "FlintExecution_" + Guid.NewGuid().ToString("N");
            var source = ManagedExecution.WriteCsharpMethod(directory, className, request.Code ?? "",
                "using System; using UnityEngine; using UnityEditor;");
            var assembly = Path.Combine(directory, className + ".dll");
            var builder = Activator.CreateInstance(UnityRuntime.Builder, assembly, new[] { source });
            Action unsubscribe = null;
            unsubscribe = ReflectionApi.Subscribe(UnityRuntime.Builder, builder, "buildFinished", arguments =>
            {
                try
                {
                    if (disposed) return;
                    var errors = new StringBuilder();
                    foreach (var message in (Array)arguments[1])
                        if (ReflectionApi.Member(message, "type").ToString() == "Error")
                            errors.AppendLine(ReflectionApi.Member(message, "file") + ":" +
                                ReflectionApi.Member(message, "line") + ": " + ReflectionApi.Member(message, "message"));
                    if (errors.Length != 0)
                        Finish(request, report, false, errors.ToString(), "compile_error");
                    else
                        Run(request, report, (string)arguments[0], className);
                }
                catch (Exception error) { Finish(request, report, false, error.ToString(), "execution_error"); }
                finally
                {
                    unsubscribe();
                    File.Delete(source);
                    File.Delete(assembly);
                    File.Delete(Path.ChangeExtension(assembly, ".pdb"));
                }
            });
            try
            {
                if (!(bool)UnityRuntime.Builder.GetMethod("Build").Invoke(builder, null))
                {
                    unsubscribe();
                    File.Delete(source);
                    UnityRuntime.Callbacks.Post(() => Execute(request, report));
                }
            }
            catch (Exception error)
            {
                unsubscribe();
                File.Delete(source);
                Finish(request, report, false, error.ToString(), "compile_error");
            }
        }

        private void Run(ExecutionRequest request, Action<ExecutionReport> report, string path, string className)
        {
            var stdout = new StringBuilder();
            var stderr = new StringBuilder();
            var outputLock = new object();
            var unsubscribe = ReflectionApi.Subscribe(UnityRuntime.Application, null, "logMessageReceivedThreaded", arguments =>
            {
                if (logScope.Value != request.RequestId) return;
                var type = arguments[2].ToString();
                lock (outputLock)
                    (type == "Error" || type == "Exception" || type == "Assert" ? stderr : stdout).AppendLine((string)arguments[0]);
            });
            Exception failure = null;
            try
            {
                logScope.Value = request.RequestId;
                ManagedExecution.Invoke(path, className);
            }
            catch (Exception error) { failure = error; }
            finally
            {
                logScope.Value = null;
                unsubscribe();
            }
            lock (outputLock)
                report(new ExecutionReport { Kind = "output", RequestId = request.RequestId,
                    Stdout = stdout.ToString(), Stderr = stderr.ToString() });
            Finish(request, report, failure == null, failure?.ToString(), failure == null ? null : "execution_error");
        }

        private static void Finish(ExecutionRequest request, Action<ExecutionReport> report,
            bool succeeded, string traceback, string error)
        {
            report(new ExecutionReport { Kind = "result", RequestId = request.RequestId,
                Succeeded = succeeded, Traceback = traceback, Error = error });
        }

        public void Dispose() { disposed = true; }
    }
}
