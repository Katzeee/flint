from ..server.discovery import DiscoveryServer
from .control_server import ControlServer


class App:

    def __init__(
        self,
        discovery_host: str = DiscoveryServer.DEFAULT_HOST,
        discovery_port: int = DiscoveryServer.DEFAULT_PORT,
    ) -> None:
        self._discovery = DiscoveryServer(discovery_host, discovery_port)
        self.control = ControlServer(self._discovery)

    async def run(self) -> None:
        await self._discovery.run()

    def stop(self) -> None:
        self._discovery.stop()
