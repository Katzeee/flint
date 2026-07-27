import asyncio
from dataclasses import dataclass
import logging
import os
import shlex
import subprocess
import sys
from pathlib import Path
from typing import Callable, Dict, List, Mapping, Optional

from filelock import FileLock, Timeout
from platformdirs import user_data_dir

from ..shared.constants import DEFAULT_HOST, CONTROL_API_PORT, REGISTRY_PORT

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
        registry_port: int = REGISTRY_PORT,
        popen_factory: Callable[..., subprocess.Popen] = subprocess.Popen,
    ) -> None:
        self._host = host
        self._port = port
        self._registry_port = registry_port
        self._popen_factory = popen_factory
        self._process: Optional[subprocess.Popen] = None
        self._log_path: Optional[Path] = None

    @classmethod
    def build_command(
        cls,
        runtime_root: str,
        port: int = CONTROL_API_PORT,
        registry_port: int = REGISTRY_PORT,
        env: Optional[Mapping[str, str]] = None,
    ) -> tuple:
        spec = cls.resolve_launch_spec(
            runtime_root,
            port=port,
            registry_port=registry_port,
            env=env,
        )
        return spec.command, spec.cwd

    @classmethod
    def resolve_launch_spec(
        cls,
        runtime_root: str,
        port: int = CONTROL_API_PORT,
        registry_port: int = REGISTRY_PORT,
        env: Optional[Mapping[str, str]] = None,
    ) -> BackendLaunchSpec:
        launch_env = os.environ if env is None else env
        explicit_spec = cls._explicit_launch_spec(runtime_root, launch_env)
        if explicit_spec is not None:
            return explicit_spec

        packaged_spec = cls._packaged_launch_spec(runtime_root, port, registry_port)
        if packaged_spec is not None:
            return packaged_spec

        return cls._development_launch_spec(runtime_root, port, registry_port)

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
    def _packaged_launch_spec(
        runtime_root: str,
        port: int,
        registry_port: int,
    ) -> Optional[BackendLaunchSpec]:
        backend_exe = Path(runtime_root) / PACKAGED_BACKEND_EXE_NAME
        if not backend_exe.exists():
            return None
        return BackendLaunchSpec(
            command=[
                str(backend_exe),
                "--api-port",
                str(port),
                "--registry-port",
                str(registry_port),
            ],
            cwd=runtime_root,
        )

    @staticmethod
    def _development_launch_spec(
        runtime_root: str,
        port: int,
        registry_port: int,
    ) -> BackendLaunchSpec:
        source_root = str(Path(runtime_root) / "src")
        existing_pythonpath = os.environ.get("PYTHONPATH", "")
        pythonpath = (
            source_root + os.pathsep + existing_pythonpath
            if existing_pythonpath
            else source_root
        )
        return BackendLaunchSpec(
            command=[
                sys.executable,
                "-X",
                "utf8",
                "-m",
                "python_bridge_mcp.server.backend",
                "--api-port",
                str(port),
                "--registry-port",
                str(registry_port),
            ],
            cwd=runtime_root,
            env={"PYTHONPATH": pythonpath},
        )

    @staticmethod
    def resolve_runtime_root() -> str:
        if getattr(sys, "frozen", False):
            return str(Path(sys.executable).resolve().parent)
        return str(Path(__file__).resolve().parents[3])

    @staticmethod
    def _state_dir() -> Path:
        path = Path(user_data_dir("python-bridge-mcp")) / "backend"
        path.mkdir(parents=True, exist_ok=True)
        return path

    @classmethod
    def launch_lock_path(cls, port: int, registry_port: int) -> Path:
        return cls._state_dir() / f"launch-{port}-{registry_port}.lock"

    @classmethod
    def singleton_lock_path(cls, port: int, registry_port: int) -> Path:
        return cls._state_dir() / f"instance-{port}-{registry_port}.lock"

    async def ensure_running(self) -> None:
        if await self._is_running():
            return

        loop = asyncio.get_event_loop()
        deadline = loop.time() + self.START_TIMEOUT
        launch_lock = FileLock(
            str(self.launch_lock_path(self._port, self._registry_port)),
            timeout=0,
        )

        while True:
            try:
                launch_lock.acquire(timeout=0)
                break
            except Timeout:
                if loop.time() >= deadline:
                    raise RuntimeError(
                        f"Timed out waiting for another shim to start backend "
                        f"(host={self._host!r}, port={self._port})"
                    )
                await asyncio.sleep(self.POLL_INTERVAL)

        try:
            # Another shim may have completed startup while this shim waited for
            # the cross-process launch lock.
            if await self._is_running():
                return

            process = self._start_subprocess()
            while loop.time() < deadline:
                return_code = process.poll()
                if return_code is not None:
                    raise RuntimeError(
                        f"Backend exited during startup with code {return_code}; "
                        f"see {self._log_path}"
                    )
                await asyncio.sleep(self.POLL_INTERVAL)
                if await self._is_running():
                    return

            self._terminate_process(process)
            log.error(
                "Backend failed to start within %.1fs (host=%r, port=%d, log=%s)",
                self.START_TIMEOUT,
                self._host,
                self._port,
                self._log_path,
            )
            raise RuntimeError(
                f"Backend failed to start within {self.START_TIMEOUT}s "
                f"(host={self._host!r}, port={self._port}, log={self._log_path})"
            )
        finally:
            launch_lock.release()

    async def _is_running(self) -> bool:
        from .backend_client import BackendClient
        try:
            client = BackendClient(host=self._host, port=self._port)
            return await asyncio.wait_for(client.ping(), timeout=1.0)
        except Exception:
            return False

    @staticmethod
    def _terminate_process(process: subprocess.Popen) -> None:
        if process.poll() is not None:
            return
        process.terminate()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=2)

    def _start_subprocess(self) -> subprocess.Popen:
        spec = self.resolve_launch_spec(
            self.resolve_runtime_root(),
            port=self._port,
            registry_port=self._registry_port,
        )
        log.info("Starting backend subprocess: %s", spec.command)
        self._log_path = self._state_dir() / "backend.log"
        log_handle = self._log_path.open("ab")
        kwargs = {
            "cwd": spec.cwd,
            "stdin": subprocess.DEVNULL,
            "stdout": log_handle,
            "stderr": subprocess.STDOUT,
            "start_new_session": True,
        }
        if spec.env:
            merged_env = dict(os.environ)
            merged_env.update(spec.env)
            kwargs["env"] = merged_env
        try:
            self._process = self._popen_factory(spec.command, **kwargs)
        except OSError as exc:
            raise RuntimeError(f"Failed to launch backend process {spec.command!r}: {exc}") from exc
        finally:
            log_handle.close()
        return self._process
