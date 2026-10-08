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
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void RunFn(IntPtr step);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool OutputFn(IntPtr step, IntPtr stdout, UIntPtr stdoutLength, IntPtr stderr, UIntPtr stderrLength);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void SucceedFn(IntPtr step, IntPtr resultId);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void FailFn(IntPtr step, IntPtr traceback, IntPtr error);

        private readonly IExecutor executor;
        private readonly IExecutionScheduler scheduler;
        private readonly RunFn runStep;
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
            runStep = nativeCore.Function<RunFn>("flint_step_run");
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

        private bool Post(IntPtr context, IntPtr step)
        {
            try { scheduler.Post(() => runStep(step)); return true; }
            catch (Exception error) { Console.Error.WriteLine(error); return false; }
        }

        private void Prepare(IntPtr context, IntPtr request, IntPtr step)
        {
            Task<object> task;
            try { task = executor.Prepare(Json.Read<ExecutionRequest>(NativeCore.ReadUtf8(request))); }
            catch (Exception error) { Fail(step, error); return; }
            Observe(task, step, () => succeed(step, GCHandle.ToIntPtr(GCHandle.Alloc(task.Result))));
        }

        private void Run(IntPtr context, IntPtr resultId, IntPtr step)
        {
            var preparedResult = Take(resultId);
            Task task;
            try { task = executor.Run(preparedResult, new Output(this, step, false), new Output(this, step, true)); }
            catch (Exception error) { Finish(step, preparedResult, error); return; }
            Observe(task, step, () => Finish(step, preparedResult, null), error => Finish(step, preparedResult, error));
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

        private void Finish(IntPtr step, object preparedResult, Exception failure)
        {
            try { (preparedResult as IDisposable)?.Dispose(); }
            catch (Exception error) { failure = failure ?? error; }
            if (failure == null) succeed(step, IntPtr.Zero);
            else Fail(step, failure);
        }

        // A completed Task finishes inline, so the core can run without posting another step.
        private void Observe(Task task, IntPtr step, Action done, Action<Exception> failed = null)
        {
            failed = failed ?? (error => Fail(step, error));
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

        private void Fail(IntPtr step, Exception failure)
        {
            var trace = NativeCore.Utf8(failure.ToString());
            try { fail(step, trace, IntPtr.Zero); }
            finally { Marshal.FreeHGlobal(trace); }
        }

        private void Write(IntPtr step, string text, bool stderr)
        {
            if (string.IsNullOrEmpty(text)) return;
            var data = NativeCore.Utf8(text);
            var length = (UIntPtr)Encoding.UTF8.GetByteCount(text);
            try
            {
                output(step, stderr ? IntPtr.Zero : data, stderr ? UIntPtr.Zero : length,
                    stderr ? data : IntPtr.Zero, stderr ? length : UIntPtr.Zero);
            }
            finally { Marshal.FreeHGlobal(data); }
        }

        private sealed class Output : TextWriter
        {
            private readonly ExecutionAdapter executionAdapter;
            private readonly IntPtr step;
            private readonly bool stderr;
            internal Output(ExecutionAdapter executionAdapter, IntPtr step, bool stderr) { this.executionAdapter = executionAdapter; this.step = step; this.stderr = stderr; }
            public override Encoding Encoding => Encoding.UTF8;
            public override void Write(string value) { executionAdapter.Write(step, value, stderr); }
            public override void Write(char value) { executionAdapter.Write(step, value.ToString(), stderr); }
            public override void Write(char[] buffer, int index, int count) { executionAdapter.Write(step, new string(buffer, index, count), stderr); }
        }
    }
}
