import asyncio
from dataclasses import dataclass
import logging
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Callable, Dict, List, Mapping, Optional

from ..shared.constants import DEFAULT_HOST, CONTROL_API_PORT

log = logging.getLogger(__name__)

EXPLICIT_BACKEND_COMMAND_ENV = "PYTHON_BRIDGE_BACKEND_COMMAND"
EXPLICIT_BACKEND_CWD_ENV = "PYTHON_BRIDGE_BACKEND_CWD"
PACKAGED_BACKEND_EXE_NAME = "python-bridge-mcp-backend.exe"


@dataclass(frozen=True)
class BackendLaunchSpec:
    command: List[str]
    cwd: str
    env: Optional[Dict[str, str]] = None


class BackendLauncher:
    START_TIMEOUT = 10.0
    POLL_INTERVAL = 0.1

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = CONTROL_API_PORT,
        popen_factory: Callable[..., subprocess.Popen] = subprocess.Popen,
    ) -> None:
        self._host = host
        self._port = port
        self._popen_factory = popen_factory

    @classmethod
    def build_command(
        cls,
        runtime_root: str,
        port: int = CONTROL_API_PORT,
        env: Optional[Mapping[str, str]] = None,
    ) -> tuple:
        spec = cls.resolve_launch_spec(runtime_root, port=port, env=env)
        return spec.command, spec.cwd

    @classmethod
    def resolve_launch_spec(
        cls,
        runtime_root: str,
        port: int = CONTROL_API_PORT,
        env: Optional[Mapping[str, str]] = None,
    ) -> BackendLaunchSpec:
        launch_env = os.environ if env is None else env
        explicit_spec = cls._explicit_launch_spec(runtime_root, launch_env)
        if explicit_spec is not None:
            return explicit_spec

        packaged_spec = cls._packaged_launch_spec(runtime_root, port)
        if packaged_spec is not None:
            return packaged_spec

        return cls._development_launch_spec(runtime_root, port)

    @classmethod
    def _explicit_launch_spec(
        cls,
        runtime_root: str,
        env: Mapping[str, str],
    ) -> Optional[BackendLaunchSpec]:
        raw_command = env.get(EXPLICIT_BACKEND_COMMAND_ENV, "").strip()
        if not raw_command:
            return None

        command = shlex.split(raw_command)
        if not command:
            raise ValueError("%s must not be empty." % EXPLICIT_BACKEND_COMMAND_ENV)

        cwd = env.get(EXPLICIT_BACKEND_CWD_ENV) or runtime_root
        return BackendLaunchSpec(command=command, cwd=cwd)

    @staticmethod
    def _packaged_launch_spec(runtime_root: str, port: int) -> Optional[BackendLaunchSpec]:
        backend_exe = Path(runtime_root) / PACKAGED_BACKEND_EXE_NAME
        if not backend_exe.exists():
            return None
        return BackendLaunchSpec(command=[str(backend_exe), "--api-port", str(port)], cwd=runtime_root)

    @staticmethod
    def _development_launch_spec(runtime_root: str, port: int) -> BackendLaunchSpec:
        return BackendLaunchSpec(
            command=[
                sys.executable,
                "-X",
                "utf8",
                "-m",
                "python_bridge_mcp.server.backend",
                "--api-port",
                str(port),
            ],
            cwd=runtime_root,
        )

    @staticmethod
    def resolve_runtime_root() -> str:
        if getattr(sys, "frozen", False):
            return str(Path(sys.executable).resolve().parent)
        return str(Path(__file__).resolve().parents[3])

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
        spec = self.resolve_launch_spec(self.resolve_runtime_root(), port=self._port)
        log.info("Starting backend subprocess: %s", spec.command)
        kwargs = {
            "cwd": spec.cwd,
            "stdin": subprocess.DEVNULL,
            "stdout": subprocess.DEVNULL,
            "stderr": subprocess.DEVNULL,
            "start_new_session": True,
        }
        if spec.env:
            merged_env = dict(os.environ)
            merged_env.update(spec.env)
            kwargs["env"] = merged_env
        try:
            self._popen_factory(spec.command, **kwargs)
        except OSError as exc:
            raise RuntimeError(f"Failed to launch backend process {spec.command!r}: {exc}") from exc
