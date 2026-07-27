import asyncio
import subprocess

from python_bridge_mcp.server.launcher import BackendLauncher
from python_bridge_mcp.server.backend_client import BackendClient
from python_bridge_mcp.shared.constants import CONTROL_API_PORT, REGISTRY_PORT
from conftest import free_port


def test_dev_backend_command_reuses_sys_executable(monkeypatch):
    monkeypatch.setattr("sys.executable", r"C:\python\python.exe")
    command, cwd = BackendLauncher.build_command(r"D:\codes\python-bridge-mcp")
    assert command == [
        r"C:\python\python.exe",
        "-X",
        "utf8",
        "-m",
        "python_bridge_mcp.server.backend",
        "--api-port",
        str(CONTROL_API_PORT),
        "--registry-port",
        str(REGISTRY_PORT),
    ]
    assert cwd == r"D:\codes\python-bridge-mcp"


def test_build_command_uses_explicit_backend_command():
    env = {
        "PYTHON_BRIDGE_BACKEND_COMMAND": r'"D:\bundle\backend.exe" --api-port 7012',
        "PYTHON_BRIDGE_BACKEND_CWD": r"D:\bundle",
    }
    command, cwd = BackendLauncher.build_command(r"D:\codes\python-bridge-mcp", env=env)
    assert command == [r"D:\bundle\backend.exe", "--api-port", "7012"]
    assert cwd == r"D:\bundle"


def test_build_command_prefers_backend_exe_in_runtime_dir(tmp_path):
    (tmp_path / "python-bridge-mcp-backend.exe").write_text("", encoding="utf-8")
    command, cwd = BackendLauncher.build_command(str(tmp_path), port=7012, env={})
    assert command == [
        str(tmp_path / "python-bridge-mcp-backend.exe"),
        "--api-port",
        "7012",
        "--registry-port",
        str(REGISTRY_PORT),
    ]
    assert cwd == str(tmp_path)


class FakePopen:
    def __init__(self, command, **kwargs):
        self.command = command
        self.kwargs = kwargs

    def poll(self):
        return None


def test_start_backend_process_detaches_stdio(monkeypatch):
    processes = []

    def fake_popen(command, **kwargs):
        process = FakePopen(command, **kwargs)
        processes.append(process)
        return process

    monkeypatch.setattr(
        BackendLauncher,
        "resolve_runtime_root",
        staticmethod(lambda: r"D:\codes\python-bridge-mcp"),
    )

    BackendLauncher(popen_factory=fake_popen)._start_subprocess()

    process = processes[0]
    assert process.command
    assert process.kwargs["stdin"] == subprocess.DEVNULL
    assert process.kwargs["stdout"].name.endswith("backend.log")
    assert process.kwargs["stderr"] == subprocess.STDOUT


def test_concurrent_ensure_running_starts_backend_once(monkeypatch, tmp_path):
    state = {"running": False, "starts": 0}

    async def fake_is_running(self):
        running = state["running"]
        await asyncio.sleep(0)
        return running

    def fake_start(self):
        state["starts"] += 1
        state["running"] = True
        return FakePopen([])

    monkeypatch.setattr(BackendLauncher, "_is_running", fake_is_running)
    monkeypatch.setattr(BackendLauncher, "_start_subprocess", fake_start)
    monkeypatch.setattr(
        BackendLauncher,
        "_state_dir",
        staticmethod(lambda: tmp_path),
    )
    monkeypatch.setattr(BackendLauncher, "POLL_INTERVAL", 0.001)

    async def run_both():
        await asyncio.gather(
            BackendLauncher(port=7012, registry_port=7013).ensure_running(),
            BackendLauncher(port=7012, registry_port=7013).ensure_running(),
        )

    asyncio.run(run_both())
    assert state["starts"] == 1


def test_concurrent_launchers_start_one_real_ready_backend(monkeypatch, tmp_path):
    api_port = free_port()
    registry_port = free_port()
    monkeypatch.setattr(
        BackendLauncher,
        "_state_dir",
        staticmethod(lambda: tmp_path),
    )
    first = BackendLauncher(port=api_port, registry_port=registry_port)
    second = BackendLauncher(port=api_port, registry_port=registry_port)

    async def start_and_ping():
        await asyncio.gather(first.ensure_running(), second.ensure_running())
        return await BackendClient(port=api_port).ping()

    try:
        assert asyncio.run(start_and_ping()) is True
        assert sum(p is not None for p in (first._process, second._process)) == 1
    finally:
        process = first._process or second._process
        if process is not None:
            BackendLauncher._terminate_process(process)


def test_launcher_restarts_backend_after_process_death(monkeypatch, tmp_path):
    api_port = free_port()
    registry_port = free_port()
    monkeypatch.setattr(
        BackendLauncher,
        "_state_dir",
        staticmethod(lambda: tmp_path),
    )
    launcher = BackendLauncher(port=api_port, registry_port=registry_port)

    async def start() -> int:
        await launcher.ensure_running()
        assert launcher._process is not None
        return launcher._process.pid

    first_pid = asyncio.run(start())
    assert launcher._process is not None
    BackendLauncher._terminate_process(launcher._process)

    try:
        second_pid = asyncio.run(start())
        assert second_pid != first_pid
        assert asyncio.run(BackendClient(port=api_port).ping()) is True
    finally:
        if launcher._process is not None:
            BackendLauncher._terminate_process(launcher._process)
