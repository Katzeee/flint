import asyncio
import uuid
from typing import Dict, Optional

from .registry import ClientEntry, Registry
from ..shared.exec_models import ExecRequest, ExecResult
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel


class ControlServer:

    DEFAULT_CONNECT_TIMEOUT: float = 10.0

    def __init__(self, discovery: Registry) -> None:
        self._discovery = discovery

    async def list_clients(self) -> Dict[str, ClientEntry]:
        return await self._discovery.list_clients()

    async def set_alias(self, instance_id: str, alias: Optional[str]) -> None:
        await self._discovery.set_alias(instance_id, alias)

    async def execute(
        self,
        instance_id: str,
        code: str,
        *,
        connect_timeout: float = DEFAULT_CONNECT_TIMEOUT,
    ) -> ExecResult:
        entry = await self._discovery.get_client(instance_id)
        if entry is None:
            raise KeyError(f"unknown client: {instance_id}")

        request_id = str(uuid.uuid4())
        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(entry.exec_host, entry.exec_port),
            timeout=connect_timeout,
        )
        try:
            req = ExecRequest(request_id=request_id, code=code)
            await AsyncJsonLineCodec.send(writer, req.to_dict())
            data = await AsyncJsonLineCodec.recv(reader)
            result = VersionedWireModel.parse_versioned(data)
            if not isinstance(result, ExecResult):
                raise RuntimeError(
                    f"unexpected response: {type(result).__name__}"
                )
            return result
        finally:
            writer.close()
