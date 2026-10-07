using System;
using System.Collections.Concurrent;
using System.Threading;

namespace Flint.Bridge
{
    /// <summary>A dedicated execution thread for hosts without an application loop.</summary>
    public sealed class WorkerThread : IExecutionScheduler
    {
        private readonly BlockingCollection<Action> callbacks = new BlockingCollection<Action>();

        public WorkerThread()
        {
            new Thread(Serve) { IsBackground = true, Name = "flint-execution" }.Start();
        }

        public void Post(Action callback) { callbacks.Add(callback); }
        public void Dispose() { callbacks.CompleteAdding(); }

        private void Serve()
        {
            foreach (var callback in callbacks.GetConsumingEnumerable()) callback();
        }
    }
}
