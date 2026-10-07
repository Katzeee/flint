using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;

namespace Flint.Bridge
{
    /// <summary>Translates the core's host callbacks into managed calls and Task completions.</summary>
    internal sealed class ExecutionBinding
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool PostFn(IntPtr context, IntPtr ticket);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void PrepareFn(IntPtr context, IntPtr request, IntPtr step);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void RunFn(IntPtr context, IntPtr prepared, IntPtr step);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void DiscardFn(IntPtr context, IntPtr prepared);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void ReleaseFn(IntPtr context);

        [StructLayout(LayoutKind.Sequential)]
        internal struct Host
        {
            internal IntPtr Context;
            private PostFn post;
            private PrepareFn prepare;
            private RunFn run;
            private DiscardFn discard;
            private ReleaseFn release;

            internal Host(ExecutionBinding binding, IntPtr context)
            {
                Context = context;
                post = binding.Post;
                prepare = binding.Prepare;
                run = binding.Run;
                discard = Discard;
                release = _ => { released = binding; binding.root.Free(); };
            }
        }

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void TicketFn(IntPtr ticket);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool OutputFn(IntPtr step, IntPtr stdout, UIntPtr stdoutLength, IntPtr stderr, UIntPtr stderrLength);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void SucceedFn(IntPtr step, IntPtr prepared);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void FailFn(IntPtr step, IntPtr traceback, IntPtr error);

        private readonly IExecutor executor;
        private readonly IExecutionScheduler scheduler;
        private readonly TicketFn runTicket;
        private readonly OutputFn output;
        private readonly SucceedFn succeed;
        private readonly FailFn fail;
        private GCHandle root;
        // A released binding outlives its own release call, whose delegate must stay alive mid-call.
        private static ExecutionBinding released;
        internal readonly Host Callbacks;

        // The core keeps this registration until it calls release.
        internal ExecutionBinding(NativeCore core, ExecutionCapabilities capabilities)
        {
            executor = capabilities.Executor;
            scheduler = capabilities.Scheduler;
            runTicket = core.Function<TicketFn>("flint_ticket_run");
            output = core.Function<OutputFn>("flint_step_output");
            succeed = core.Function<SucceedFn>("flint_step_succeed");
            fail = core.Function<FailFn>("flint_step_fail");
            root = GCHandle.Alloc(this);
            Callbacks = new Host(this, GCHandle.ToIntPtr(root));
        }

        private bool Post(IntPtr context, IntPtr ticket)
        {
            try { scheduler.Post(() => runTicket(ticket)); return true; }
            catch (Exception error) { Console.Error.WriteLine(error); return false; }
        }

        private void Prepare(IntPtr context, IntPtr request, IntPtr pointer)
        {
            var step = new Step(this, pointer);
            Task<object> task;
            try { task = executor.Prepare(Json.Read<ExecutionRequest>(NativeCore.ReadUtf8(request))); }
            catch (Exception error) { step.Fail(error); return; }
            Observe(task, step, () => step.Succeed(GCHandle.ToIntPtr(GCHandle.Alloc(task.Result))));
        }

        private void Run(IntPtr context, IntPtr prepared, IntPtr pointer)
        {
            var step = new Step(this, pointer);
            var value = Take(prepared);
            Task task;
            try { task = executor.Run(value, new Output(step, false), new Output(step, true)); }
            catch (Exception error) { Finish(step, value, error); return; }
            Observe(task, step, () => Finish(step, value, null), error => Finish(step, value, error));
        }

        private static void Discard(IntPtr context, IntPtr prepared)
        {
            try { (Take(prepared) as IDisposable)?.Dispose(); }
            catch (Exception error) { Console.Error.WriteLine(error); }
        }

        private static object Take(IntPtr prepared)
        {
            var handle = GCHandle.FromIntPtr(prepared);
            var value = handle.Target;
            handle.Free();
            return value;
        }

        private static void Finish(Step step, object prepared, Exception failure)
        {
            try { (prepared as IDisposable)?.Dispose(); }
            catch (Exception error) { failure = failure ?? error; }
            if (failure == null) step.Succeed(IntPtr.Zero);
            else step.Fail(failure);
        }

        // A completed Task finishes inline, so the core can run without another ticket.
        private static void Observe(Task task, Step step, Action done, Action<Exception> failed = null)
        {
            failed = failed ?? step.Fail;
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

        private sealed class Step
        {
            private readonly ExecutionBinding binding;
            private readonly object gate = new object();
            private IntPtr pointer;
            internal Step(ExecutionBinding binding, IntPtr pointer) { this.binding = binding; this.pointer = pointer; }

            internal void Write(string text, bool stderr)
            {
                if (string.IsNullOrEmpty(text)) return;
                var data = NativeCore.Utf8(text);
                var length = (UIntPtr)Encoding.UTF8.GetByteCount(text);
                try
                {
                    lock (gate)
                    {
                        if (pointer == IntPtr.Zero) return;
                        binding.output(pointer, stderr ? IntPtr.Zero : data, stderr ? UIntPtr.Zero : length,
                            stderr ? data : IntPtr.Zero, stderr ? length : UIntPtr.Zero);
                    }
                }
                finally { Marshal.FreeHGlobal(data); }
            }

            internal void Succeed(IntPtr prepared)
            {
                var taken = Take();
                if (taken != IntPtr.Zero) binding.succeed(taken, prepared);
            }

            internal void Fail(Exception failure)
            {
                var taken = Take();
                if (taken == IntPtr.Zero) return;
                var trace = NativeCore.Utf8(failure.ToString());
                try { binding.fail(taken, trace, IntPtr.Zero); }
                finally { Marshal.FreeHGlobal(trace); }
            }

            private IntPtr Take()
            {
                lock (gate)
                {
                    var taken = pointer;
                    pointer = IntPtr.Zero;
                    return taken;
                }
            }
        }

        private sealed class Output : TextWriter
        {
            private readonly Step step;
            private readonly bool stderr;
            internal Output(Step step, bool stderr) { this.step = step; this.stderr = stderr; }
            public override Encoding Encoding => Encoding.UTF8;
            public override void Write(string value) { step.Write(value, stderr); }
            public override void Write(char value) { step.Write(value.ToString(), stderr); }
            public override void Write(char[] buffer, int index, int count) { step.Write(new string(buffer, index, count), stderr); }
        }
    }
}
