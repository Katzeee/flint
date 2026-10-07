using System;
using System.IO;
using System.Threading.Tasks;

namespace Flint.Bridge
{
    /// <summary>
    /// Prepare turns a request into the value Run executes. Either Task may complete
    /// later. A prepared value that is IDisposable is disposed after Run completes or
    /// when the Bridge stops before running it.
    /// </summary>
    public interface IExecutor
    {
        Task<object> Prepare(ExecutionRequest request);
        Task Run(object prepared, TextWriter stdout, TextWriter stderr);
    }

    /// <summary>Runs callbacks on the host's execution thread; disposed after the Bridge is released.</summary>
    public interface IExecutionScheduler : IDisposable
    {
        void Post(Action callback);
    }

    public sealed class ExecutionCapabilities
    {
        public readonly IExecutor Executor;
        public readonly IExecutionScheduler Scheduler;
        public ExecutionCapabilities(IExecutor executor, IExecutionScheduler scheduler)
        {
            Executor = executor ?? throw new ArgumentNullException(nameof(executor));
            Scheduler = scheduler ?? throw new ArgumentNullException(nameof(scheduler));
        }
    }
}
