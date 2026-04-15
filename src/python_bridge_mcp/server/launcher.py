import asyncio
import os
import subprocess
import sys


class BackendLauncher:
    DEFAULT_HOST = "localhost"
    DEFAULT_PORT = 6322
    START_TIMEOUT = 10.0
    POLL_INTERVAL = 0.1

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
    ) -> None:
        self._host = host
        self._port = port

    async def ensure_running(self) -> None:
        if await self._is_running():
            return
        self._start_subprocess()
        loop = asyncio.get_event_loop()
        deadline = loop.time() + self.START_TIMEOUT
        while loop.time() < deadline:
            await asyncio.sleep(self.POLL_INTERVAL)
            if await self._is_running():
                return
        raise RuntimeError(
            f"Backend failed to start within {self.START_TIMEOUT}s "
            f"(host={self._host!r}, port={self._port})"
        )

    async def _is_running(self) -> bool:
        try:
            _, writer = await asyncio.wait_for(
                asyncio.open_connection(self._host, self._port),
                timeout=0.5,
            )
            writer.close()
            return True
        except (OSError, asyncio.TimeoutError):
            return False

    def _start_subprocess(self) -> None:
        custom = os.environ.get("PYTHON_BRIDGE_BACKEND_COMMAND")
        if custom:
            args = custom.split()
        else:
            args = [
                sys.executable, "-m", "python_bridge_mcp.server.backend",
                "--api-port", str(self._port),
            ]
        subprocess.Popen(args, start_new_session=True)
