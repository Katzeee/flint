using System;
using System.Collections.Concurrent;

namespace Flint.Bridge
{
    /// <summary>A host drains posted callbacks from its own event loop, which outlives any one Bridge.</summary>
    public sealed class CallbackQueue : IExecutionScheduler
    {
        private readonly ConcurrentQueue<Action> pending = new ConcurrentQueue<Action>();
        public void Post(Action callback) { pending.Enqueue(callback); }
        public void Drain()
        {
            int count = pending.Count;
            while (count-- > 0 && pending.TryDequeue(out var callback)) callback();
        }
        public void Dispose() { }
    }
}
