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

    public sealed class BridgeStoppedException : InvalidOperationException
    {
        public BridgeStoppedException() : base("Bridge has stopped; finish disconnect before starting another Bridge") { }
    }

    /// <summary>
    /// Loads the native core and creates it with the host's execution capabilities.
    /// A created core keeps its library loaded so outstanding tickets and steps stay callable.
    /// </summary>
    public sealed class NativeCore : IDisposable
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
        private delegate IntPtr CreateFn(IntPtr config, [In] ref ExecutionBinding.Host host, out uint errorKind, out IntPtr errorMessage);
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
        private readonly StatusFn _stop;
        private readonly StatusFn _connected;
        private readonly StatusFn _busy;
        private readonly StatusFn _stopped;
        private readonly InstanceIdFn _instanceId;
        private readonly InstanceIdFn _statusJson;
        private readonly StatusFn _reconnect;
        private readonly ApplySettingsFn _applySettings;
        private readonly HandleFn _destroy;
        private readonly StringFreeFn _stringFree;

        public NativeCore(string libraryPath, string configJson, ExecutionCapabilities capabilities)
        {
            if (libraryPath == null) throw new ArgumentNullException(nameof(libraryPath));
            if (configJson == null) throw new ArgumentNullException(nameof(configJson));
            if (capabilities == null) throw new ArgumentNullException(nameof(capabilities));
            _module = LoadLibraryW(libraryPath);
            if (_module == IntPtr.Zero)
            {
                int error = Marshal.GetLastWin32Error();
                throw new BridgeCreationException(BridgeCreationErrorKind.LibraryUnavailable,
                    "Cannot load Bridge core: " + libraryPath + " (" + new Win32Exception(error).Message + ")");
            }
            try
            {
                if (Function<AbiVersionFn>("flint_bridge_abi_version")() != 6)
                    throw new BridgeCreationException(BridgeCreationErrorKind.AbiMismatch, "Unsupported native Bridge ABI");
                _create = Function<CreateFn>("flint_bridge_create");
                _stop = Function<StatusFn>("flint_bridge_stop");
                _connected = Function<StatusFn>("flint_bridge_connected");
                _busy = Function<StatusFn>("flint_bridge_busy");
                _stopped = Function<StatusFn>("flint_bridge_stopped");
                _instanceId = Function<InstanceIdFn>("flint_bridge_instance_id");
                _statusJson = Function<InstanceIdFn>("flint_bridge_status_json");
                _reconnect = Function<StatusFn>("flint_bridge_reconnect");
                _applySettings = Function<ApplySettingsFn>("flint_bridge_apply_settings");
                _destroy = Function<HandleFn>("flint_bridge_destroy");
                _stringFree = Function<StringFreeFn>("flint_bridge_string_free");
                var host = new ExecutionBinding(this, capabilities).Callbacks;
                IntPtr config = Utf8(configJson);
                uint errorKind;
                IntPtr errorMessage;
                try { _handle = _create(config, ref host, out errorKind, out errorMessage); }
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

        internal T Function<T>(string name) where T : class
        {
            IntPtr address = GetProcAddress(_module, name);
            if (address == IntPtr.Zero)
                throw new BridgeCreationException(BridgeCreationErrorKind.AbiMismatch, "Bridge core is missing " + name);
            return (T)(object)Marshal.GetDelegateForFunctionPointer(address, typeof(T));
        }

        internal static IntPtr Utf8(string value)
        {
            byte[] bytes = Encoding.UTF8.GetBytes(value + "\0");
            IntPtr buffer = Marshal.AllocHGlobal(bytes.Length);
            Marshal.Copy(bytes, 0, buffer, bytes.Length);
            return buffer;
        }

        internal static string ReadUtf8(IntPtr value)
        {
            if (value == IntPtr.Zero) return null;
            int length = 0;
            while (Marshal.ReadByte(value, length) != 0) length++;
            byte[] bytes = new byte[length];
            Marshal.Copy(value, bytes, 0, length);
            return Encoding.UTF8.GetString(bytes);
        }
        internal string TakeString(IntPtr value)
        {
            try { return ReadUtf8(value); }
            finally { if (value != IntPtr.Zero) _stringFree(value); }
        }

        internal IntPtr Handle
        {
            get
            {
                if (_handle == IntPtr.Zero) throw new ObjectDisposedException(nameof(NativeCore));
                return _handle;
            }
        }

        public bool Connected { get { return _connected(Handle); } }
        public bool Busy { get { return _busy(Handle); } }
        public string InstanceId { get { return TakeString(_instanceId(Handle)); } }
        public string StatusJson { get { return TakeString(_statusJson(Handle)); } }
        /// <summary>Ends the connection; false while started host code is still active.</summary>
        public bool Stop() { return _handle == IntPtr.Zero || _stop(_handle); }

        public void Reconnect() { if (!_reconnect(Handle)) throw new BridgeStoppedException(); }
        public void CheckRunning() { if (_stopped(Handle)) throw new BridgeStoppedException(); }
        public void ApplySettings(string settingsJson)
        {
            if (settingsJson == null) throw new ArgumentNullException(nameof(settingsJson));
            IntPtr settings = Utf8(settingsJson);
            try
            {
                uint result = _applySettings(Handle, settings);
                if (result == 1) throw new BridgeBusyException();
                if (result == 2) throw new ArgumentException("Invalid Bridge connection settings", nameof(settingsJson));
                if (result == 3) throw new BridgeStoppedException();
                if (result != 0) throw new InvalidOperationException("Unknown Bridge settings result");
            }
            finally { Marshal.FreeHGlobal(settings); }
        }

        /// <summary>Destroys the core; call after Stop returns true unless the hosting context is ending.</summary>
        public void Dispose()
        {
            if (_handle != IntPtr.Zero)
            {
                _destroy(_handle);
                _handle = IntPtr.Zero;
            }
        }
    }
}
