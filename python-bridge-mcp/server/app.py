from .registry import Registry
from .control_server import ControlServer


class App:

    def __init__(
        self,
        discovery_host: str = Registry.DEFAULT_HOST,
        discovery_port: int = Registry.DEFAULT_PORT,
    ) -> None:
        self._discovery = Registry(discovery_host, discovery_port)
        self.control = ControlServer(self._discovery)

    async def run(self) -> None:
        await self._discovery.run()

    def stop(self) -> None:
        self._discovery.stop()
