using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;

namespace Flint.Bridge
{
    /// <summary>Translates the core's host callbacks into managed calls and Task completions.</summary>
    internal sealed class ExecutionAdapter
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void TicketFn(IntPtr ticket);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool OutputFn(IntPtr raw, IntPtr stdout, UIntPtr stdoutLength, IntPtr stderr, UIntPtr stderrLength);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void SucceedFn(IntPtr raw, IntPtr resultId);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void FailFn(IntPtr raw, IntPtr traceback, IntPtr error);

        private readonly IExecutor executor;
        private readonly IExecutionScheduler scheduler;
        private readonly TicketFn runTicket;
        private readonly OutputFn output;
        private readonly SucceedFn succeed;
        private readonly FailFn fail;
        private GCHandle root;
        // A released adapter outlives its own release call, whose delegate must stay alive mid-call.
        private static ExecutionAdapter releasedExecutionAdapter;
        internal readonly ExecutionBinding ExecutionBinding;

        // The core keeps this registration until it calls release.
        internal ExecutionAdapter(NativeCore nativeCore, ExecutionCapabilities executionCapabilities)
        {
            executor = executionCapabilities.Executor;
            scheduler = executionCapabilities.Scheduler;
            runTicket = nativeCore.Function<TicketFn>("flint_ticket_run");
            output = nativeCore.Function<OutputFn>("flint_step_output");
            succeed = nativeCore.Function<SucceedFn>("flint_step_succeed");
            fail = nativeCore.Function<FailFn>("flint_step_fail");
            root = GCHandle.Alloc(this);
            ExecutionBinding = new ExecutionBinding {
                Context = GCHandle.ToIntPtr(root),
                Post = Post,
                Prepare = Prepare,
                Run = Run,
                Discard = Discard,
                Release = _ => { releasedExecutionAdapter = this; root.Free(); }
            };
        }

        private bool Post(IntPtr context, IntPtr ticket)
        {
            try { scheduler.Post(() => runTicket(ticket)); return true; }
            catch (Exception error) { Console.Error.WriteLine(error); return false; }
        }

        private void Prepare(IntPtr context, IntPtr request, IntPtr raw)
        {
            var stepWrapper = new StepWrapper(this, raw);
            Task<object> task;
            try { task = executor.Prepare(Json.Read<ExecutionRequest>(NativeCore.ReadUtf8(request))); }
            catch (Exception error) { stepWrapper.Fail(error); return; }
            Observe(task, stepWrapper, () => stepWrapper.Succeed(GCHandle.ToIntPtr(GCHandle.Alloc(task.Result))));
        }

        private void Run(IntPtr context, IntPtr resultId, IntPtr raw)
        {
            var stepWrapper = new StepWrapper(this, raw);
            var preparedResult = Take(resultId);
            Task task;
            try { task = executor.Run(preparedResult, new Output(stepWrapper, false), new Output(stepWrapper, true)); }
            catch (Exception error) { Finish(stepWrapper, preparedResult, error); return; }
            Observe(task, stepWrapper, () => Finish(stepWrapper, preparedResult, null), error => Finish(stepWrapper, preparedResult, error));
        }

        private static void Discard(IntPtr context, IntPtr resultId)
        {
            try { (Take(resultId) as IDisposable)?.Dispose(); }
            catch (Exception error) { Console.Error.WriteLine(error); }
        }

        private static object Take(IntPtr resultId)
        {
            var handle = GCHandle.FromIntPtr(resultId);
            var preparedResult = handle.Target;
            handle.Free();
            return preparedResult;
        }

        private static void Finish(StepWrapper stepWrapper, object preparedResult, Exception failure)
        {
            try { (preparedResult as IDisposable)?.Dispose(); }
            catch (Exception error) { failure = failure ?? error; }
            if (failure == null) stepWrapper.Succeed(IntPtr.Zero);
            else stepWrapper.Fail(failure);
        }

        // A completed Task finishes inline, so the core can run without another ticket.
        private static void Observe(Task task, StepWrapper stepWrapper, Action done, Action<Exception> failed = null)
        {
            failed = failed ?? stepWrapper.Fail;
            if (task == null) { failed(new InvalidOperationException("The executor returned no Task")); return; }
            Action<Task> settle = completed =>
            {
                if (completed.IsFaulted) failed(completed.Exception.InnerException ?? completed.Exception);
                else if (completed.IsCanceled) failed(new TaskCanceledException(completed));
                else done();
            };
            if (task.IsCompleted) settle(task);
            else task.ContinueWith(settle, TaskContinuationOptions.ExecuteSynchronously);
        }

        private sealed class StepWrapper
        {
            private readonly ExecutionAdapter executionAdapter;
            private readonly object gate = new object();
            private IntPtr raw;
            internal StepWrapper(ExecutionAdapter executionAdapter, IntPtr raw) { this.executionAdapter = executionAdapter; this.raw = raw; }

            internal void Write(string text, bool stderr)
            {
                if (string.IsNullOrEmpty(text)) return;
                var data = NativeCore.Utf8(text);
                var length = (UIntPtr)Encoding.UTF8.GetByteCount(text);
                try
                {
                    lock (gate)
                    {
                        if (raw == IntPtr.Zero) return;
                        executionAdapter.output(raw, stderr ? IntPtr.Zero : data, stderr ? UIntPtr.Zero : length,
                            stderr ? data : IntPtr.Zero, stderr ? length : UIntPtr.Zero);
                    }
                }
                finally { Marshal.FreeHGlobal(data); }
            }

            internal void Succeed(IntPtr resultId)
            {
                var taken = Take();
                if (taken != IntPtr.Zero) executionAdapter.succeed(taken, resultId);
            }

            internal void Fail(Exception failure)
            {
                var taken = Take();
                if (taken == IntPtr.Zero) return;
                var trace = NativeCore.Utf8(failure.ToString());
                try { executionAdapter.fail(taken, trace, IntPtr.Zero); }
                finally { Marshal.FreeHGlobal(trace); }
            }

            private IntPtr Take()
            {
                lock (gate)
                {
                    var taken = raw;
                    raw = IntPtr.Zero;
                    return taken;
                }
            }
        }

        private sealed class Output : TextWriter
        {
            private readonly StepWrapper stepWrapper;
            private readonly bool stderr;
            internal Output(StepWrapper stepWrapper, bool stderr) { this.stepWrapper = stepWrapper; this.stderr = stderr; }
            public override Encoding Encoding => Encoding.UTF8;
            public override void Write(string value) { stepWrapper.Write(value, stderr); }
            public override void Write(char value) { stepWrapper.Write(value.ToString(), stderr); }
            public override void Write(char[] buffer, int index, int count) { stepWrapper.Write(new string(buffer, index, count), stderr); }
        }
    }
}
