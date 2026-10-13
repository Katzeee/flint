using System;
using System.Reflection;
using System.Runtime.Serialization;

namespace Flint.Bridge
{
    [DataContract]
    internal sealed class MonoPlan
    {
        [DataMember(Name = "entry")] public string Entry { get; set; }
        [DataMember(Name = "native_library")] public string NativeLibrary { get; set; }
        [DataMember(Name = "settings")] public HostSettings Settings { get; set; }
    }

    /// <summary>Routes native bootstrap calls to the host's attach and restore entries.</summary>
    public static class Attach
    {
        public static string Initialize(string request)
        {
            try
            {
                var attach = Json.Read<MonoPlan>(request);
                Invoke(attach.Entry, "Attach", new object[] { attach.NativeLibrary, attach.Settings });
                return null;
            }
            catch (Exception error) { return error.ToString(); }
        }

        // Restore only schedules host work; it must not wait for the host loop
        // while the native caller holds a reference to the domain.
        public static string Restore(string entry)
        {
            try
            {
                Invoke(entry, "Restore", new object[0]);
                return null;
            }
            catch (Exception error) { return error.ToString(); }
        }

        private static void Invoke(string entry, string method, object[] arguments)
        {
            var type = typeof(Attach).Assembly.GetType(entry, true);
            var target = type.GetMethod(method, BindingFlags.Public | BindingFlags.Static);
            if (target == null) throw new MissingMethodException(entry, method);
            target.Invoke(null, arguments);
        }
    }
}
