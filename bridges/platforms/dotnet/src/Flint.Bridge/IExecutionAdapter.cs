using System;

namespace Flint.Bridge
{
    public interface IExecutionAdapter : IDisposable
    {
        void Execute(ExecutionRequest request, Action<ExecutionReport> report);
    }
}
