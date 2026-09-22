import struct
from uuid import uuid4

from flint_protocol.framing import MAX_FRAME_BYTES, encode_frame, validate
from flint_protocol.v1.envelope_pb2 import Envelope


def message(kind, payload, request_id=None):
    envelope = Envelope(protocol_version=1, request_id=request_id or uuid4().hex)
    getattr(envelope, kind).CopyFrom(payload)
    return envelope


async def receive(reader):
    size = struct.unpack(">I", await reader.readexactly(4))[0]
    if not 0 < size <= MAX_FRAME_BYTES:
        raise ValueError("Invalid frame length")
    result = Envelope()
    result.ParseFromString(await reader.readexactly(size))
    validate(result)
    return result


async def send(writer, envelope):
    writer.write(encode_frame(envelope))
    await writer.drain()
