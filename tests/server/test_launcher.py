import subprocess

from python_bridge_mcp.server.launcher import BackendLauncher


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
        "6322",
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
    (tmp_path / "backend.exe").write_text("", encoding="utf-8")
    command, cwd = BackendLauncher.build_command(str(tmp_path), port=7012, env={})
    assert command == [str(tmp_path / "backend.exe"), "--api-port", "7012"]
    assert cwd == str(tmp_path)


class FakePopen:
    def __init__(self, command, **kwargs):
        self.command = command
        self.kwargs = kwargs


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
    assert process.kwargs["stdout"] == subprocess.DEVNULL
    assert process.kwargs["stderr"] == subprocess.DEVNULL
