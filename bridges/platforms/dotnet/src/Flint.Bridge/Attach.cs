using System;
using System.Reflection;
using System.Runtime.Serialization;

namespace Flint.Bridge
{
    [DataContract]
    internal sealed class AttachRequest
    {
        [DataMember(Name = "manager")] public string Manager { get; set; }
        [DataMember(Name = "native_library")] public string NativeLibrary { get; set; }
        [DataMember(Name = "address")] public string Address { get; set; }
        [DataMember(Name = "port")] public int Port { get; set; }
        [DataMember(Name = "name")] public string Name { get; set; }
    }

    /// <summary>The attach bootstrap's managed entry: attach the Bridge of the requested host type's static Manager.</summary>
    public static class Attach
    {
        public static string Initialize(string request)
        {
            try
            {
                var attach = Json.Read<AttachRequest>(request);
                var type = typeof(Attach).Assembly.GetType(attach.Manager, true);
                var property = type.GetProperty("Manager", BindingFlags.Public | BindingFlags.Static);
                var manager = property?.GetValue(null) as BridgeManager;
                if (manager == null) throw new MissingMemberException(attach.Manager, "Manager");
                manager.Attach(attach.NativeLibrary, new BridgeSettings
                {
                    Address = attach.Address,
                    Port = attach.Port,
                    Name = attach.Name,
                    Enabled = true
                });
                return null;
            }
            catch (Exception error) { return error.ToString(); }
        }
    }
}
