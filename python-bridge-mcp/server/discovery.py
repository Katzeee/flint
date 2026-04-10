import asyncio
from dataclasses import dataclass
from typing import Dict, Optional

from ..shared.discovery_models import AckDiscovery, HeartbeatDiscovery, RegisterDiscovery
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError


@dataclass
class ClientEntry:
    pid: str
    instance_id: str
    instance_name: str
    exec_host: str
    exec_port: int
    alias: Optional[str]


class DiscoveryServer:
    DEFAULT_HOST = "localhost"
    DEFAULT_PORT = 6321

    def __init__(self, host: str = DEFAULT_HOST, port: int = DEFAULT_PORT):
        self._host = host
        self._port = port
        self._clients: Dict[str, ClientEntry] = {}
        self._server: Optional[asyncio.AbstractServer] = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    @property
    def clients(self) -> Dict[str, ClientEntry]:
        return dict(self._clients)

    async def run(self) -> None:
        self._server = await asyncio.start_server(
            self._handle_client, self._host, self._port
        )
        async with self._server:
            await self._server.serve_forever()

    def stop(self) -> None:
        if self._server is not None:
            self._server.close()

    # ------------------------------------------------------------------
    # Connection handler
    # ------------------------------------------------------------------

    async def _handle_client(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        instance_id: Optional[str] = None
        try:
            while True:
                try:
                    data = await asyncio.wait_for(AsyncJsonLineCodec.recv(reader), timeout=30)
                    msg = VersionedWireModel.parse_versioned(data)
                except asyncio.TimeoutError:
                    return  # no heartbeat in time — drop connection
                except (ConnectionError, WireModelError, ValueError) as e:
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error=str(e)).to_dict())
                    return

                if isinstance(msg, RegisterDiscovery):
                    instance_id = msg.instance_id
                    self._clients[instance_id] = ClientEntry(
                        pid=msg.pid,
                        instance_id=msg.instance_id,
                        instance_name=msg.instance_name,
                        exec_host=msg.exec_host,
                        exec_port=msg.exec_port,
                        alias=msg.alias,
                    )
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=True).to_dict())

                elif isinstance(msg, HeartbeatDiscovery):
                    if msg.instance_id not in self._clients:
                        await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error="not registered").to_dict())
                        return
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=True).to_dict())

                else:
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error="unexpected message type").to_dict())
                    return
        finally:
            if instance_id is not None:
                self._clients.pop(instance_id, None)
            writer.close()
