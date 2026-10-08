using System;
using System.Runtime.InteropServices;

namespace Flint.Bridge
{
    /// <summary>The host execution binding passed to the native Bridge core.</summary>
    [StructLayout(LayoutKind.Sequential)]
    internal struct ExecutionBinding
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] [return: MarshalAs(UnmanagedType.I1)]
        internal delegate bool PostFn(IntPtr context, IntPtr step);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void PrepareFn(IntPtr context, IntPtr request, IntPtr step);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void RunFn(IntPtr context, IntPtr resultId, IntPtr step);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void DiscardFn(IntPtr context, IntPtr resultId);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] internal delegate void ReleaseFn(IntPtr context);

        internal IntPtr Context;
        internal PostFn Post;
        internal PrepareFn Prepare;
        internal RunFn Run;
        internal DiscardFn Discard;
        internal ReleaseFn Release;
    }
}
