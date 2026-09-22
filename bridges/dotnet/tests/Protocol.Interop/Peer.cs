using System;
using System.IO;
using System.Net.Sockets;
using Flint.Protocol;

class Peer
{
    static void Main(string[] args)
    {
        using (var client = new TcpClient("127.0.0.1", int.Parse(args[0])))
        using (var stream = client.GetStream())
        using (var input = File.OpenRead(args[1]))
        using (var output = File.Create(args[2]))
        {
            client.ReceiveTimeout = 5000; client.SendTimeout = 5000;
            while (true)
            {
                var message = Framing.Read(input);
                if (message == null) break;
                var frame = Framing.Encode(message);
                stream.Write(frame, 0, frame.Length);
                var response = Framing.Read(stream);
                if (response == null) throw new EndOfStreamException();
                var returned = Framing.Encode(response);
                output.Write(returned, 0, returned.Length);
            }
        }
    }
}
