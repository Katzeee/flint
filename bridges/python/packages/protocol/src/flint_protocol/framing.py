"""Blocking socket framing; callers own scheduling, deadlines and connection closure."""
import struct

from .v1.envelope_pb2 import Envelope

PROTOCOL_VERSION = 1
MAX_FRAME_BYTES = 100 * 1024 * 1024


def validate(envelope):
    if envelope.protocol_version != PROTOCOL_VERSION:
        raise ValueError("unsupported protocol version")
    if not envelope.request_id:
        raise ValueError("missing request_id")
    if envelope.WhichOneof("payload") is None:
        raise ValueError("missing or unsupported payload")


def encode_frame(envelope, limit=MAX_FRAME_BYTES):
    validate(envelope)
    if not 0 < limit <= MAX_FRAME_BYTES:
        raise ValueError("invalid frame limit")
    if envelope.ByteSize() > limit:
        raise ValueError("frame exceeds limit")
    payload = envelope.SerializeToString()
    return struct.pack(">I", len(payload)) + payload


def _read_exact(connection, count, allow_eof=False):
    result = bytearray()
    while len(result) < count:
        chunk = connection.recv(count - len(result))
        if not chunk:
            if not result and allow_eof:
                return None
            raise EOFError("truncated frame")
        result.extend(chunk)
    return bytes(result)


def read_frame(connection, limit=MAX_FRAME_BYTES):
    """Return None only for EOF between complete frames. Close on any error."""
    if not 0 < limit <= MAX_FRAME_BYTES:
        raise ValueError("invalid frame limit")
    header = _read_exact(connection, 4, allow_eof=True)
    if header is None:
        return None
    length = struct.unpack(">I", header)[0]
    if not 0 < length <= limit:
        raise ValueError("invalid frame length")
    envelope = Envelope()
    envelope.ParseFromString(_read_exact(connection, length))
    validate(envelope)
    return envelope


def write_frame(connection, envelope, limit=MAX_FRAME_BYTES):
    connection.sendall(encode_frame(envelope, limit))
