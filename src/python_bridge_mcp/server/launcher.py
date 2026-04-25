import asyncio
import logging
import os
import subprocess
import sys
from pathlib import Path

log = logging.getLogger(__name__)


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
        log.error(
            "Backend failed to start within %.1fs (host=%r, port=%d)",
            self.START_TIMEOUT, self._host, self._port,
        )
        raise RuntimeError(
            f"Backend failed to start within {self.START_TIMEOUT}s "
            f"(host={self._host!r}, port={self._port})"
        )

    async def _is_running(self) -> bool:
        from .backend_client import BackendClient
        try:
            client = BackendClient(host=self._host, port=self._port)
            return await asyncio.wait_for(client.ping(), timeout=1.0)
        except Exception:
            return False

    def _start_subprocess(self) -> None:
        custom = os.environ.get("PYTHON_BRIDGE_BACKEND_COMMAND")
        if custom:
            args = custom.split()
        else:
            args = [
                sys.executable, "-X", "utf8", "-m", "python_bridge_mcp.server.backend",
                "--api-port", str(self._port),
            ]
        log.info("Starting backend subprocess: %s", args)
        try:
            subprocess.Popen(
                args,
                cwd=str(Path(__file__).resolve().parents[3]),
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
        except OSError as exc:
            raise RuntimeError(f"Failed to launch backend process {args!r}: {exc}") from exc
