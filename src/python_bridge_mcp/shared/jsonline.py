import asyncio
import json
import socket
from typing import Any, Dict, Optional


class JsonLineCodec:
    """Encode/decode newline-delimited JSON messages (stateless helpers)."""

    @staticmethod
    def encode(data: Dict[str, Any]) -> bytes:
        return (json.dumps(data) + "\n").encode("utf-8")

    @staticmethod
    def decode(raw: bytes) -> Dict[str, Any]:
        if not raw:
            raise ConnectionError("Connection closed by peer")
        return json.loads(raw.decode("utf-8"))


class SyncJsonLineCodec:
    """Framed JSON-line read/write over a blocking socket."""

    SYNC_READER_LIMIT = 100 * 1024 * 1024  # 100 MB

    @staticmethod
    def send(conn: socket.socket, data: Dict[str, Any]) -> None:
        conn.sendall(JsonLineCodec.encode(data))

    @staticmethod
    def recv(conn: socket.socket) -> Dict[str, Any]:
        buf = b""
        while b"\n" not in buf:
            chunk = conn.recv(4096)
            if not chunk:
                raise ConnectionError("Connection closed by peer")
            buf += chunk
            if len(buf) > SyncJsonLineCodec.SYNC_READER_LIMIT:
                raise ConnectionError(
                    f"Incoming message exceeds {SyncJsonLineCodec.SYNC_READER_LIMIT} bytes"
                )
        line = buf.split(b"\n", 1)[0]
        return JsonLineCodec.decode(line)


class AsyncJsonLineCodec:
    """Framed JSON-line read/write over asyncio streams."""

    READER_LIMIT = 100 * 1024 * 1024  # 100 MB

    @staticmethod
    async def send(writer: asyncio.StreamWriter, data: Dict[str, Any]) -> None:
        writer.write(JsonLineCodec.encode(data))
        await writer.drain()

    @staticmethod
    async def recv(reader: asyncio.StreamReader, timeout: Optional[float] = None) -> Dict[str, Any]:
        if timeout is not None:
            line = await asyncio.wait_for(reader.readline(), timeout)
        else:
            line = await reader.readline()
        return JsonLineCodec.decode(line)
