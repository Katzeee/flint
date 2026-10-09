using System;
using System.Diagnostics;
using System.Threading;
using System.Threading.Tasks;

namespace Flint.Bridge
{
    /// <summary>Creates, configures, and releases a fixed host's Bridge.</summary>
    public sealed class BridgeManager : IDisposable
    {
        private readonly object gate = new object();
        private readonly string host;
        private readonly Func<string> runtimeVersion;
        private readonly Func<ExecutionCapabilities> createExecution;
        private readonly Action<Action> dispatch;
        private NativeCore bridge;
        private IExecutionScheduler scheduler;
        private string library;
        private readonly EventHandler unload;
        private bool disposed;

        public BridgeManager(string host, Func<string> runtimeVersion,
            Func<ExecutionCapabilities> createExecution, Action<Action> dispatch)
        {
            this.host = host;
            this.runtimeVersion = runtimeVersion;
            this.createExecution = createExecution;
            this.dispatch = dispatch;
            unload = (_, __) => Dispose();
            AppDomain.CurrentDomain.DomainUnload += unload;
        }

        public bool Connected { get { lock (gate) return bridge != null && bridge.Connected; } }
        public bool Busy { get { lock (gate) return bridge != null && bridge.Busy; } }
        public string StatusJson { get { lock (gate) return bridge?.StatusJson; } }
        public string InstanceId { get { lock (gate) return bridge?.InstanceId; } }

        public void Connect(string nativeLibrary, BridgeSettings settings)
        {
            lock (gate)
            {
                if (disposed) throw new ObjectDisposedException(nameof(BridgeManager));
                if (bridge != null)
                {
                    bridge.CheckRunning();
                    var current = Json.Read<BridgeStatus>(bridge.StatusJson).Settings;
                    if (current.Address != settings.Address || current.Port != settings.Port)
                        throw new InvalidOperationException("Disconnect the existing Bridge before changing its endpoint");
                    return;
                }
                library = nativeLibrary;
                var capabilities = createExecution();
                try
                {
                    bridge = new NativeCore(library, Json.Write(new BridgeConfiguration
                    {
                        Host = host,
                        RuntimeVersion = runtimeVersion(),
                        Address = settings.Address,
                        Port = settings.Port,
                        Name = settings.Name,
                        Enabled = settings.Enabled
                    }), capabilities);
                }
                catch { capabilities.Scheduler.Dispose(); throw; }
                scheduler = capabilities.Scheduler;
            }
        }

        public void Configure(BridgeSettings settings, string nativeLibrary = null)
        {
            lock (gate)
            {
                if (bridge == null)
                {
                    var path = nativeLibrary ?? library;
                    if (path == null) throw new InvalidOperationException("Bridge has no native library path");
                    Connect(path, settings);
                    return;
                }
                bridge.ApplySettings(Json.Write(settings));
            }
        }

        /// <summary>Dispatch initialization once; cancellation only prevents work that has not begun.</summary>
        public string Attach(string nativeLibrary, BridgeSettings settings, int timeoutMilliseconds = 20000)
        {
            var elapsed = Stopwatch.StartNew();
            // 0 = queued, 1 = started, 2 = cancelled. The dispatch callback can
            // outlive this wait, so it must not reference a disposed wait handle.
            int state = 0;
            var completion = new TaskCompletionSource<bool>();
            try
            {
                dispatch(() =>
                {
                    if (Interlocked.CompareExchange(ref state, 1, 0) != 0) return;
                    try { Configure(settings, nativeLibrary); completion.SetResult(true); }
                    catch (Exception error) { completion.SetException(error); }
                });
            }
            catch { Interlocked.CompareExchange(ref state, 2, 0); throw; }
            try
            {
                if (!completion.Task.Wait(Math.Max(0, timeoutMilliseconds - (int)elapsed.ElapsedMilliseconds)))
                {
                    if (Interlocked.CompareExchange(ref state, 2, 0) == 0)
                        throw new TimeoutException("Bridge initialization timed out and was cancelled before starting");
                    throw new TimeoutException("Bridge initialization is still in progress; connection outcome is unknown");
                }
            }
            catch (AggregateException error) { throw error.InnerException; }
            while (settings.Enabled && !Connected && elapsed.ElapsedMilliseconds < timeoutMilliseconds)
                Thread.Sleep(20);
            if (settings.Enabled && !Connected) throw new TimeoutException("Bridge registration did not complete");
            return InstanceId;
        }

        public void Reconnect()
        {
            lock (gate)
            {
                if (bridge == null) throw new InvalidOperationException("No Bridge has been started");
                bridge.Reconnect();
            }
        }

        /// <summary>Releases the Bridge once no started host code remains; false means retry later.</summary>
        public bool Disconnect()
        {
            lock (gate)
            {
                if (bridge == null) return true;
                if (!bridge.Stop()) return false;
                Release();
                return true;
            }
        }

        /// <summary>The hosting context is ending: release the Bridge even if host code has not finished.</summary>
        public void Dispose()
        {
            lock (gate)
            {
                if (disposed) return;
                disposed = true;
                AppDomain.CurrentDomain.DomainUnload -= unload;
                if (bridge != null) Release();
            }
        }

        private void Release()
        {
            bridge.Dispose();
            scheduler.Dispose();
            bridge = null;
            scheduler = null;
        }
    }
}
