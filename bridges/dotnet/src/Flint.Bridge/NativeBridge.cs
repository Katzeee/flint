using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;

namespace Flint.Bridge
{
    /// <summary>
    /// Loads the shared connection core. The host adapter polls execute events and
    /// submits output and results after dispatching code on its required thread.
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
        private delegate IntPtr CreateFn(IntPtr config);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr PollFn(IntPtr handle, uint timeoutMilliseconds);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool SubmitFn(IntPtr handle, IntPtr command);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        [return: MarshalAs(UnmanagedType.I1)]
        private delegate bool StatusFn(IntPtr handle);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate IntPtr InstanceIdFn(IntPtr handle);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void HandleFn(IntPtr handle);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)]
        private delegate void StringFreeFn(IntPtr value);

        private IntPtr _module;
        private IntPtr _handle;
        private readonly CreateFn _create;
        private readonly PollFn _poll;
        private readonly SubmitFn _submit;
        private readonly StatusFn _connected;
        private readonly StatusFn _busy;
        private readonly InstanceIdFn _instanceId;
        private readonly HandleFn _reconnect;
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
                throw new Win32Exception(error, "Cannot load Bridge core: " + libraryPath + " (Win32 " + error + ")");
            }
            try
            {
                if (Function<AbiVersionFn>("flint_bridge_abi_version")() != 1)
                    throw new InvalidOperationException("Unsupported native Bridge ABI");
                _create = Function<CreateFn>("flint_bridge_create");
                _poll = Function<PollFn>("flint_bridge_poll");
                _submit = Function<SubmitFn>("flint_bridge_submit");
                _connected = Function<StatusFn>("flint_bridge_connected");
                _busy = Function<StatusFn>("flint_bridge_busy");
                _instanceId = Function<InstanceIdFn>("flint_bridge_instance_id");
                _reconnect = Function<HandleFn>("flint_bridge_reconnect");
                _stop = Function<HandleFn>("flint_bridge_stop");
                _destroy = Function<HandleFn>("flint_bridge_destroy");
                _stringFree = Function<StringFreeFn>("flint_bridge_string_free");
                IntPtr config = Utf8(configJson);
                try { _handle = _create(config); }
                finally { Marshal.FreeHGlobal(config); }
                if (_handle == IntPtr.Zero) throw new ArgumentException("Invalid Bridge configuration", nameof(configJson));
            }
            catch
            {
                FreeLibrary(_module);
                _module = IntPtr.Zero;
                throw;
            }
        }

        private T Function<T>(string name) where T : class
        {
            IntPtr address = GetProcAddress(_module, name);
            if (address == IntPtr.Zero) throw new EntryPointNotFoundException(name);
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
        public string Poll(uint timeoutMilliseconds) { return TakeString(_poll(Handle, timeoutMilliseconds)); }

        public bool Submit(string commandJson)
        {
            if (commandJson == null) throw new ArgumentNullException(nameof(commandJson));
            IntPtr command = Utf8(commandJson);
            try { return _submit(Handle, command); }
            finally { Marshal.FreeHGlobal(command); }
        }

        public void Reconnect() { _reconnect(Handle); }
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
