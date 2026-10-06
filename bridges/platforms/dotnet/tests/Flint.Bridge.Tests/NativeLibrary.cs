using System;

namespace Flint.Bridge.Tests
{
    internal static class NativeLibrary
    {
        internal static string Path
        {
            get
            {
                var path = Environment.GetEnvironmentVariable("FLINT_BRIDGE_CORE");
                if (string.IsNullOrEmpty(path) || !System.IO.File.Exists(path))
                    throw new InvalidOperationException(
                        "FLINT_BRIDGE_CORE must point to the native core built by `cargo build --locked -p flint-bridge-core`; `cargo xtask test csharp` sets it");
                return path;
            }
        }

    }
}
