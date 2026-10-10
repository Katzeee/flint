using System.IO;
using System.Runtime.Serialization;
using System.Runtime.Serialization.Json;
using System.Text;

namespace Flint.Bridge
{
    [DataContract]
    public class HostSettings
    {
        [DataMember(Name = "address")] public string Address { get; set; }
        [DataMember(Name = "port")] public int Port { get; set; }
        [DataMember(Name = "name")] public string Name { get; set; }
    }

    [DataContract]
    internal sealed class BridgeConfiguration
    {
        [DataMember(Name = "host")] public string Host { get; set; }
        [DataMember(Name = "settings")] public HostSettings Settings { get; set; }
        [DataMember(Name = "runtime_version")] public string RuntimeVersion { get; set; }
    }

    [DataContract]
    internal sealed class BridgeStatus
    {
        [DataMember(Name = "settings")] public HostSettings Settings { get; set; }
    }

    [DataContract]
    public sealed class ExecutionRequest
    {
        [DataMember(Name = "request_id")] public string RequestId { get; set; }
        [DataMember(Name = "code")] public string Code { get; set; }
    }

    internal static class Json
    {
        public static string Write<T>(T value)
        {
            using (var stream = new MemoryStream())
            {
                new DataContractJsonSerializer(typeof(T)).WriteObject(stream, value);
                return Encoding.UTF8.GetString(stream.ToArray());
            }
        }

        public static T Read<T>(string value)
        {
            using (var stream = new MemoryStream(Encoding.UTF8.GetBytes(value)))
                return (T)new DataContractJsonSerializer(typeof(T)).ReadObject(stream);
        }
    }
}
