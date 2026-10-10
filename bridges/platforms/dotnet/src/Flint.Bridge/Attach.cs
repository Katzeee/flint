using System;
using System.Reflection;
using System.Runtime.Serialization;

namespace Flint.Bridge
{
    [DataContract]
    internal sealed class MonoPlan
    {
        [DataMember(Name = "manager")] public string Manager { get; set; }
        [DataMember(Name = "native_library")] public string NativeLibrary { get; set; }
        [DataMember(Name = "settings")] public HostSettings Settings { get; set; }
    }

    /// <summary>The attach bootstrap's managed entry: attach the Bridge of the requested host type's static Manager.</summary>
    public static class Attach
    {
        public static string Initialize(string request)
        {
            try
            {
                var attach = Json.Read<MonoPlan>(request);
                var type = typeof(Attach).Assembly.GetType(attach.Manager, true);
                var property = type.GetProperty("Manager", BindingFlags.Public | BindingFlags.Static);
                var manager = property?.GetValue(null) as BridgeManager;
                if (manager == null) throw new MissingMemberException(attach.Manager, "Manager");
                manager.Attach(attach.NativeLibrary, attach.Settings);
                return null;
            }
            catch (Exception error) { return error.ToString(); }
        }
    }
}
