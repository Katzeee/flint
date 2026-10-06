using System;
using System.Collections.Concurrent;
using System.Threading;

namespace Flint.Bridge
{
    /// <summary>Owns the executor, request pump, and native core binding.</summary>
    internal sealed class Bridge : IDisposable
    {
        internal readonly NativeCore Core;
        private readonly IExecutionAdapter executor;
        private readonly Action<Action> dispatch;
        private readonly Thread poller;
        private readonly object reports = new object();
        private readonly ConcurrentDictionary<string, Invocation> pending = new ConcurrentDictionary<string, Invocation>();

        private sealed class Invocation
        {
            internal readonly ExecutionRequest Request;
            private int state;
            internal Invocation(ExecutionRequest request) { Request = request; }
            internal bool Start() { return Interlocked.CompareExchange(ref state, 1, 0) == 0; }
            internal bool Cancel() { return Interlocked.CompareExchange(ref state, 2, 0) == 0; }
        }
        private volatile bool stopping;
        private bool disposed;

        internal Bridge(string library, BridgeConfiguration config,
            IExecutionAdapter executor, Action<Action> dispatch)
        {
            this.executor = executor;
            this.dispatch = dispatch;
            poller = new Thread(Poll) { IsBackground = true, Name = "flint-bridge" };
            Core = new NativeCore(library, Json.Write(config));
            try { poller.Start(); }
            catch { Core.Dispose(); throw; }
        }

        private void Poll()
        {
            while (!stopping)
            {
                var message = Core.Poll(100);
                if (message == null) continue;
                var request = Json.Read<ExecutionRequest>(message);
                var invocation = new Invocation(request);
                pending[request.RequestId] = invocation;
                try
                {
                    dispatch(() =>
                    {
                        if (!invocation.Start()) return;
                        try { executor.Execute(request, Report); }
                        catch (Exception error) { Reject(request, error.ToString()); }
                    });
                }
                catch (Exception error) { Reject(request, error.ToString()); }
            }
        }

        private void Report(ExecutionReport report)
        {
            lock (reports)
            {
                if (!disposed) Core.ReportExecution(Json.Write(report));
                if (report.Kind == "result") pending.TryRemove(report.RequestId, out _);
            }
        }

        private void Reject(ExecutionRequest request, string error)
        {
            Report(new ExecutionReport { Kind = "result", RequestId = request.RequestId,
                Succeeded = false, Error = error });
        }

        internal bool Stop()
        {
            stopping = true;
            Core.Stop();
            // Host dispatch must enqueue work without waiting for its execution.
            if (Thread.CurrentThread != poller) poller.Join();
            foreach (var invocation in pending.Values)
                if (invocation.Cancel()) Reject(invocation.Request, "Bridge stopped before host execution");
            return !Core.Busy;
        }

        public void Dispose()
        {
            Stop();
            try { executor.Dispose(); }
            finally
            {
                lock (reports)
                {
                    disposed = true;
                    Core.Dispose();
                }
            }
        }
    }
}
