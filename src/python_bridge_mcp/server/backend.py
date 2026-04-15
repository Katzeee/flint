import argparse
import asyncio

from .control_api import ControlApi
from .control_server import ControlServer
from .registry import Registry


async def main(
    *,
    registry_host: str = Registry.DEFAULT_HOST,
    registry_port: int = Registry.DEFAULT_PORT,
    api_host: str = ControlApi.DEFAULT_HOST,
    api_port: int = ControlApi.DEFAULT_PORT,
) -> None:
    registry = Registry(host=registry_host, port=registry_port)
    control = ControlServer(registry)
    api = ControlApi(control, host=api_host, port=api_port)
    await asyncio.gather(registry.run(), api.run())


def _cli() -> None:
    parser = argparse.ArgumentParser(description="python-bridge-mcp backend process")
    parser.add_argument("--registry-port", type=int, default=Registry.DEFAULT_PORT)
    parser.add_argument("--api-port", type=int, default=ControlApi.DEFAULT_PORT)
    args = parser.parse_args()
    asyncio.run(main(registry_port=args.registry_port, api_port=args.api_port))


if __name__ == "__main__":
    _cli()
