import argparse
import asyncio
import logging

from filelock import FileLock, Timeout

from ..shared.constants import DEFAULT_HOST, REGISTRY_BIND_HOST, REGISTRY_PORT, CONTROL_API_PORT
from .control_server import ControlServer
from .launcher import BackendLauncher
from .registry import Registry

log = logging.getLogger(__name__)


async def main(
    *,
    registry_host: str = REGISTRY_BIND_HOST,
    registry_port: int = REGISTRY_PORT,
    api_host: str = DEFAULT_HOST,
    api_port: int = CONTROL_API_PORT,
) -> None:
    singleton_lock = FileLock(
        str(BackendLauncher.singleton_lock_path(api_port, registry_port)),
        timeout=0,
    )
    try:
        singleton_lock.acquire(timeout=0)
    except Timeout as exc:
        raise RuntimeError(
            f"Backend already running for control={api_port}, registry={registry_port}"
        ) from exc

    registry = Registry(host=registry_host, port=registry_port)
    control = ControlServer(registry, host=api_host, port=api_port, ready=False)
    log.info(
        "Backend starting (registry=%s:%d, control=%s:%d)",
        registry_host, registry_port, api_host, api_port,
    )
    try:
        # Bind both listeners before advertising readiness. Binding registry
        # first also prevents two backend processes from splitting ownership of
        # the registry and control ports.
        await registry.start()
        try:
            await control.start()
        except Exception:
            registry.stop()
            await registry.wait_closed()
            raise
        control.set_ready(True)
        await asyncio.gather(registry.serve(), control.serve())
    finally:
        control.set_ready(False)
        registry.stop()
        control.stop()
        await asyncio.gather(
            registry.wait_closed(),
            control.wait_closed(),
            return_exceptions=True,
        )
        singleton_lock.release()


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
