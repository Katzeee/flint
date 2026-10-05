using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace Flint.Bridge
{
    /// <summary>Why no Bridge was created.</summary>
    public enum BridgeCreationErrorKind
    {
        InvalidConfiguration,
        /// <summary>Another Bridge owns this process.</summary>
        Claimed,
        System,
        LibraryUnavailable,
        AbiMismatch
    }

    /// <summary>No Bridge was created; <see cref="Kind"/> names why.</summary>
    public sealed class BridgeCreationException : Exception
    {
        public BridgeCreationException(BridgeCreationErrorKind kind, string message) : base(message)
        {
            Kind = kind;
        }

        public BridgeCreationErrorKind Kind { get; }
    }

    /// <summary>The Bridge refused a settings change while host code is executing.</summary>
    public sealed class BridgeBusyException : InvalidOperationException
    {
        public BridgeBusyException() : base("Bridge is executing host code") { }
    }

    /// <summary>
    /// Loads the shared connection core. The host adapter polls execute events and
    /// reports output and results after dispatching code on its required thread.
    /// </summary>
    public sealed class NativeBridge : IDisposable
    {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryW(string path);

        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
        private static extern IntPtr GetProcAddress(IntPtr module, string name);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool FreeLibrary(IntPtr module);

        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate uint AbiVersionFn();
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr CreateFn(IntPtr config, out uint errorKind, out IntPtr errorMessage);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr PollFn(IntPtr handle, uint timeoutMilliseconds);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool ReportExecutionFn(IntPtr handle, IntPtr report);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool StatusFn(IntPtr handle);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr InstanceIdFn(IntPtr handle);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void HandleFn(IntPtr handle);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate uint ApplySettingsFn(IntPtr handle, IntPtr settings);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void StringFreeFn(IntPtr value);

        private IntPtr _module;
        private IntPtr _handle;
        private readonly CreateFn _create;
        private readonly PollFn _poll;
        private readonly ReportExecutionFn _reportExecution;
        private readonly StatusFn _connected;
        private readonly StatusFn _busy;
        private readonly InstanceIdFn _instanceId;
        private readonly InstanceIdFn _statusJson;
        private readonly HandleFn _reconnect;
        private readonly ApplySettingsFn _applySettings;
        private readonly HandleFn _stop;
        private readonly HandleFn _destroy;
        private readonly StringFreeFn _stringFree;

        public NativeBridge(string libraryPath, string configJson)
        {
            if (libraryPath == null) throw new ArgumentNullException(nameof(libraryPath));
            if (configJson == null) throw new ArgumentNullException(nameof(configJson));
            _module = LoadLibraryW(libraryPath);
            if (_module == IntPtr.Zero)
            {
                int error = Marshal.GetLastWin32Error();
                throw new BridgeCreationException(BridgeCreationErrorKind.LibraryUnavailable,
                    "Cannot load Bridge core: " + libraryPath + " (" + new Win32Exception(error).Message + ")");
            }
            try
            {
                if (Function<AbiVersionFn>("flint_bridge_abi_version")() != 3)
                    throw new BridgeCreationException(BridgeCreationErrorKind.AbiMismatch, "Unsupported native Bridge ABI");
                _create = Function<CreateFn>("flint_bridge_create");
                _poll = Function<PollFn>("flint_bridge_poll");
                _reportExecution = Function<ReportExecutionFn>("flint_bridge_report_execution");
                _connected = Function<StatusFn>("flint_bridge_connected");
                _busy = Function<StatusFn>("flint_bridge_busy");
                _instanceId = Function<InstanceIdFn>("flint_bridge_instance_id");
                _statusJson = Function<InstanceIdFn>("flint_bridge_status_json");
                _reconnect = Function<HandleFn>("flint_bridge_reconnect");
                _applySettings = Function<ApplySettingsFn>("flint_bridge_apply_settings");
                _stop = Function<HandleFn>("flint_bridge_stop");
                _destroy = Function<HandleFn>("flint_bridge_destroy");
                _stringFree = Function<StringFreeFn>("flint_bridge_string_free");
                IntPtr config = Utf8(configJson);
                uint errorKind;
                IntPtr errorMessage;
                try { _handle = _create(config, out errorKind, out errorMessage); }
                finally { Marshal.FreeHGlobal(config); }
                string message = TakeString(errorMessage);
                if (_handle == IntPtr.Zero)
                    throw new BridgeCreationException(CreationErrorKind(errorKind), message ?? "Cannot start native Bridge core");
            }
            catch
            {
                FreeLibrary(_module);
                _module = IntPtr.Zero;
                throw;
            }
        }

        // Creation error codes from the native core.
        private static BridgeCreationErrorKind CreationErrorKind(uint code)
        {
            switch (code)
            {
                case 1: return BridgeCreationErrorKind.InvalidConfiguration;
                case 2: return BridgeCreationErrorKind.Claimed;
                default: return BridgeCreationErrorKind.System;
            }
        }

        private T Function<T>(string name) where T : class
        {
            IntPtr address = GetProcAddress(_module, name);
            if (address == IntPtr.Zero)
                throw new BridgeCreationException(BridgeCreationErrorKind.AbiMismatch, "Bridge core is missing " + name);
            return (T)(object)Marshal.GetDelegateForFunctionPointer(address, typeof(T));
        }

        private static IntPtr Utf8(string value)
        {
            byte[] bytes = Encoding.UTF8.GetBytes(value + "\0");
            IntPtr buffer = Marshal.AllocHGlobal(bytes.Length);
            Marshal.Copy(bytes, 0, buffer, bytes.Length);
            return buffer;
        }

        private string TakeString(IntPtr value)
        {
            if (value == IntPtr.Zero) return null;
            try
            {
                int length = 0;
                while (Marshal.ReadByte(value, length) != 0) length++;
                byte[] bytes = new byte[length];
                Marshal.Copy(value, bytes, 0, length);
                return Encoding.UTF8.GetString(bytes);
            }
            finally { _stringFree(value); }
        }

        private IntPtr Handle
        {
            get
            {
                if (_handle == IntPtr.Zero) throw new ObjectDisposedException(nameof(NativeBridge));
                return _handle;
            }
        }

        public bool Connected { get { return _connected(Handle); } }
        public bool Busy { get { return _busy(Handle); } }
        public string InstanceId { get { return TakeString(_instanceId(Handle)); } }
        public string StatusJson { get { return TakeString(_statusJson(Handle)); } }
        public string Poll(uint timeoutMilliseconds) { return TakeString(_poll(Handle, timeoutMilliseconds)); }

        public bool ReportExecution(string reportJson)
        {
            if (reportJson == null) throw new ArgumentNullException(nameof(reportJson));
            IntPtr report = Utf8(reportJson);
            try { return _reportExecution(Handle, report); }
            finally { Marshal.FreeHGlobal(report); }
        }

        public void Reconnect() { _reconnect(Handle); }
        public void ApplySettings(string settingsJson)
        {
            if (settingsJson == null) throw new ArgumentNullException(nameof(settingsJson));
            IntPtr settings = Utf8(settingsJson);
            try
            {
                uint result = _applySettings(Handle, settings);
                if (result == 1) throw new BridgeBusyException();
                if (result == 2) throw new ArgumentException("Invalid Bridge connection settings", nameof(settingsJson));
                if (result != 0) throw new InvalidOperationException("Unknown Bridge settings result");
            }
            finally { Marshal.FreeHGlobal(settings); }
        }
        public void Stop() { _stop(Handle); }

        public void Dispose()
        {
            if (_handle != IntPtr.Zero)
            {
                _destroy(_handle);
                _handle = IntPtr.Zero;
            }
            if (_module != IntPtr.Zero)
            {
                FreeLibrary(_module);
                _module = IntPtr.Zero;
            }
            GC.SuppressFinalize(this);
        }
    }
}
