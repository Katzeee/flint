import asyncio
import uuid
from typing import Optional

from ..shared.exec_models import ExecRequest, ExecResult
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel


class ExecCaller:
    """Connects to a client's exec port, sends ExecRequest, returns ExecResult."""

    DEFAULT_CONNECT_TIMEOUT: float = 10.0

    @staticmethod
    async def call(
        host: str,
        port: int,
        code: str,
        *,
        request_id: Optional[str] = None,
        connect_timeout: float = DEFAULT_CONNECT_TIMEOUT,
    ) -> ExecResult:
        if request_id is None:
            request_id = str(uuid.uuid4())

        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(host, port),
            timeout=connect_timeout,
        )
        try:
            req = ExecRequest(request_id=request_id, code=code)
            await AsyncJsonLineCodec.send(writer, req.to_dict())
            data = await AsyncJsonLineCodec.recv(reader)
            result = VersionedWireModel.parse_versioned(data)
            if not isinstance(result, ExecResult):
                raise RuntimeError(f"unexpected response: {type(result).__name__}")
            return result
        finally:
            writer.close()

    @classmethod
    async def call_instance(
        cls,
        entry,
        code: str,
        **kwargs,
    ) -> ExecResult:
        return await cls.call(entry.exec_host, entry.exec_port, code, **kwargs)
