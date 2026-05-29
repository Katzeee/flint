import argparse
import asyncio
import logging

from ..shared.constants import DEFAULT_HOST, REGISTRY_BIND_HOST, REGISTRY_PORT, CONTROL_API_PORT
from .control_server import ControlServer
from .registry import Registry

log = logging.getLogger(__name__)


async def main(
    *,
    registry_host: str = REGISTRY_BIND_HOST,
    registry_port: int = REGISTRY_PORT,
    api_host: str = DEFAULT_HOST,
    api_port: int = CONTROL_API_PORT,
) -> None:
    registry = Registry(host=registry_host, port=registry_port)
    control = ControlServer(registry, host=api_host, port=api_port)
    log.info(
        "Backend starting (registry=%s:%d, control=%s:%d)",
        registry_host, registry_port, api_host, api_port,
    )
    await asyncio.gather(registry.run(), control.run())


def _cli() -> None:
    parser = argparse.ArgumentParser(description="python-bridge-mcp backend process")
    parser.add_argument("--registry-host", default=REGISTRY_BIND_HOST)
    parser.add_argument("--registry-port", type=int, default=REGISTRY_PORT)
    parser.add_argument("--api-port", type=int, default=CONTROL_API_PORT)
    args = parser.parse_args()
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(name)s] %(levelname)s %(message)s",
    )
    asyncio.run(main(registry_host=args.registry_host, registry_port=args.registry_port, api_port=args.api_port))


if __name__ == "__main__":
    _cli()
