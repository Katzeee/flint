using System;
using System.IO;
using Google.Protobuf;
using Flint.Protocol.V1;

namespace Flint.Protocol
{
    /// <summary>Blocking stream framing. The caller owns deadlines and closes on errors.</summary>
    public static class Framing
    {
        public const uint ProtocolVersion = 1;
        public const int MaxFrameBytes = 100 * 1024 * 1024;

        public static void Validate(Envelope envelope)
        {
            if (envelope.ProtocolVersion != ProtocolVersion) throw new InvalidDataException("unsupported protocol version");
            if (envelope.RequestId.Length == 0) throw new InvalidDataException("missing request_id");
            if (envelope.PayloadCase == Envelope.PayloadOneofCase.None) throw new InvalidDataException("missing or unsupported payload");
        }

        public static byte[] Encode(Envelope envelope, int limit = MaxFrameBytes)
        {
            CheckLimit(limit);
            Validate(envelope);
            int length = envelope.CalculateSize();
            if (length > limit) throw new InvalidDataException("frame exceeds limit");
            byte[] frame = new byte[length + 4];
            frame[0] = (byte)(length >> 24); frame[1] = (byte)(length >> 16);
            frame[2] = (byte)(length >> 8); frame[3] = (byte)length;
            byte[] payload = envelope.ToByteArray();
            Buffer.BlockCopy(payload, 0, frame, 4, length);
            return frame;
        }

        public static Envelope Read(Stream stream, int limit = MaxFrameBytes)
        {
            CheckLimit(limit);
            byte[] header = ReadExact(stream, 4, true);
            if (header == null) return null;
            uint length = ((uint)header[0] << 24) | ((uint)header[1] << 16) | ((uint)header[2] << 8) | header[3];
            if (length == 0 || length > limit) throw new InvalidDataException("invalid frame length");
            Envelope envelope = Envelope.Parser.ParseFrom(ReadExact(stream, (int)length, false));
            Validate(envelope);
            return envelope;
        }

        private static void CheckLimit(int limit)
        {
            if (limit <= 0 || limit > MaxFrameBytes) throw new ArgumentOutOfRangeException("limit");
        }

        private static byte[] ReadExact(Stream stream, int count, bool allowEof)
        {
            byte[] bytes = new byte[count];
            int read = 0;
            while (read < count)
            {
                int received = stream.Read(bytes, read, count - read);
                if (received == 0)
                {
                    if (read == 0 && allowEof) return null;
                    throw new EndOfStreamException("truncated frame");
                }
                read += received;
            }
            return bytes;
        }
    }
}
