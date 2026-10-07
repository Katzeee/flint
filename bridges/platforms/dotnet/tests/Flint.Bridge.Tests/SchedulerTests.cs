using System;
using System.Collections.Generic;
using System.Threading;
using Xunit;

namespace Flint.Bridge.Tests
{
    public class SchedulerTests
    {
        [Fact]
        public void WorkerRunsCallbacksInOrderOnItsThreadAndRefusesAfterDisposal()
        {
            var worker = new WorkerThread();
            var calls = new List<(string, int)>();
            using (var finished = new ManualResetEventSlim())
            {
                worker.Post(() => calls.Add(("first", Thread.CurrentThread.ManagedThreadId)));
                worker.Post(() => calls.Add(("second", Thread.CurrentThread.ManagedThreadId)));
                worker.Post(finished.Set);
                worker.Dispose();
                Assert.True(finished.Wait(3000, TestContext.Current.CancellationToken));
            }
            Assert.Equal(new[] { "first", "second" }, calls.ConvertAll(call => call.Item1));
            Assert.Equal(calls[0].Item2, calls[1].Item2);
            Assert.NotEqual(Thread.CurrentThread.ManagedThreadId, calls[0].Item2);
            Assert.Throws<InvalidOperationException>(() => worker.Post(() => { }));
        }

        [Fact]
        public void QueueRunsOnlyWhatWasPostedBeforeEachDrain()
        {
            var queue = new CallbackQueue();
            var calls = new List<int>();
            queue.Post(() => { calls.Add(1); queue.Post(() => calls.Add(2)); });
            queue.Drain();
            Assert.Equal(new[] { 1 }, calls);
            queue.Drain();
            Assert.Equal(new[] { 1, 2 }, calls);
        }
    }
}
