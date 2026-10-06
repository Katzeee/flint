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
        private readonly Func<IExecutionAdapter> createExecutor;
        private readonly Action<Action> dispatch;
        private Bridge bridge;
        private string library;
        private string address;
        private int port;
        private readonly EventHandler unload;
        private bool disposed;

        public BridgeManager(string host, Func<string> runtimeVersion,
            Func<IExecutionAdapter> createExecutor, Action<Action> dispatch)
        {
            this.host = host;
            this.runtimeVersion = runtimeVersion;
            this.createExecutor = createExecutor;
            this.dispatch = dispatch;
            unload = (_, __) => Dispose();
            AppDomain.CurrentDomain.DomainUnload += unload;
        }

        public bool Connected { get { lock (gate) return bridge != null && bridge.Core.Connected; } }
        public bool Busy { get { lock (gate) return bridge != null && bridge.Core.Busy; } }
        public string StatusJson { get { lock (gate) return bridge?.Core.StatusJson; } }
        public string InstanceId { get { lock (gate) return bridge?.Core.InstanceId; } }

        public void Connect(string nativeLibrary, BridgeSettings settings)
        {
            lock (gate)
            {
                if (disposed) throw new ObjectDisposedException(nameof(BridgeManager));
                if (bridge != null)
                {
                    bridge.Core.CheckRunning();
                    if (address != settings.Address || port != settings.Port)
                        throw new InvalidOperationException("Disconnect the existing Bridge before changing its endpoint");
                    return;
                }
                library = nativeLibrary;
                var executor = createExecutor();
                try
                {
                    bridge = new Bridge(library, new BridgeConfiguration
                    {
                        Host = host, RuntimeVersion = runtimeVersion(), Address = settings.Address,
                        Port = settings.Port, Name = settings.Name, Enabled = settings.Enabled
                    }, executor, dispatch);
                }
                catch { executor.Dispose(); throw; }
                address = settings.Address;
                port = settings.Port;
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
                bridge.Core.ApplySettings(Json.Write(settings));
                address = settings.Address;
                port = settings.Port;
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
                bridge.Core.Reconnect();
            }
        }

        public bool Disconnect()
        {
            lock (gate)
            {
                if (bridge == null) return true;
                if (!bridge.Stop()) return false;
                bridge.Dispose();
                bridge = null;
                return true;
            }
        }

        /// <summary>Release the Bridge when its hosting context is being destroyed.</summary>
        public void Dispose()
        {
            lock (gate)
            {
                if (disposed) return;
                disposed = true;
                AppDomain.CurrentDomain.DomainUnload -= unload;
                bridge?.Dispose();
                bridge = null;
            }
        }
    }
}
