import asyncio
import json
import threading
from typing import Iterator, Optional

import pytest
from mcp.server.fastmcp.exceptions import ToolError

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.exec_listener import ExecListener
from python_bridge_mcp.server.app import App
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import ClientEntry, Registry
from python_bridge_mcp.server.shim import mcp as shim_mcp
from python_bridge_mcp.shared.discovery_models import RegisterDiscovery
from python_bridge_mcp.shared.jsonline import AsyncJsonLineCodec
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence

from conftest import AsyncRunner, free_port


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

EXPECTED_TOOLS = {
    "list_dcc_targets",
    "exec_python",
    "start_workflow",
    "get_workflow_overview",
    "get_workflow_execution",
    "set_target_alias",
}


def _bg_register(
    discovery_port: int,
    exec_port: int,
    instance_id: str = "c1",
    instance_name: str = "test",
    alias: Optional[str] = None,
    instance_type: str = "",
    pid: int = 1,
) -> threading.Event:
    done = threading.Event()

    def _run() -> None:
        async def _do() -> None:
            reader, writer = await asyncio.open_connection("localhost", discovery_port)
            try:
                msg = RegisterDiscovery(
                    pid=pid, instance_id=instance_id, instance_name=instance_name,
                    exec_host="localhost", exec_port=exec_port, alias=alias,
                    instance_type=instance_type,
                )
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                done.set()
                await asyncio.sleep(5)
            finally:
                writer.close()

        asyncio.run(_do())

    threading.Thread(target=_run, daemon=True).start()
    assert done.wait(timeout=3), "registration failed"
    return done


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def discovery_port() -> int:
    return free_port()


@pytest.fixture
def exec_port() -> int:
    return free_port()


@pytest.fixture
def app_runner(discovery_port: int) -> Iterator[tuple]:
    app = App(discovery_host="localhost", discovery_port=discovery_port)
    runner = AsyncRunner()
    runner.start(app.run)
    yield app, runner
    app.stop()
    runner.stop()


# ---------------------------------------------------------------------------
# E1 — basic framework
# ---------------------------------------------------------------------------

def test_shim_import_and_mcp_instance() -> None:
    """FastMCP instance exists and is importable."""
    from mcp.server.fastmcp import FastMCP
    assert isinstance(shim_mcp, FastMCP)


def test_shim_has_all_tools() -> None:
    """All 6 expected tools are registered."""
    tool_names = {t.name for t in shim_mcp._tool_manager.list_tools()}
    assert tool_names == EXPECTED_TOOLS


# ---------------------------------------------------------------------------
# E2 — list_dcc_targets tool
# ---------------------------------------------------------------------------

def test_list_dcc_targets_returns_targets(
    app_runner, discovery_port: int, exec_port: int, monkeypatch,
) -> None:
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    _bg_register(discovery_port, exec_port, instance_name="myapp", instance_type="maya")

    content, raw = asyncio.run(shim_mcp.call_tool("list_dcc_targets", {}))
    data = json.loads(content[0].text)
    assert len(data) == 1
    assert data[0]["instance_id"] == "c1"
    assert data[0]["instance_name"] == "myapp"
    assert data[0]["instance_type"] == "maya"


def test_list_dcc_targets_filters_by_type(
    app_runner, discovery_port: int, monkeypatch,
) -> None:
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    port_maya = free_port()
    port_nuke = free_port()
    _bg_register(discovery_port, port_maya, instance_id="c1", instance_type="maya", pid=1)
    _bg_register(discovery_port, port_nuke, instance_id="c2", instance_type="nuke", pid=2)

    content, raw = asyncio.run(shim_mcp.call_tool("list_dcc_targets", {"dcc_type": "maya"}))
    data = json.loads(content[0].text)
    assert len(data) == 1
    assert data[0]["instance_id"] == "c1"


# ---------------------------------------------------------------------------
# E3 — exec_python tool
# ---------------------------------------------------------------------------

@pytest.fixture
def listener_port() -> int:
    return free_port()


@pytest.fixture
def listener_runner(listener_port: int) -> Iterator[AsyncRunner]:
    executor = CodeExecutor()
    code_runner = DirectRunner(executor)
    listener = ExecListener("localhost", listener_port, code_runner)
    lr = AsyncRunner()
    lr.start(listener.run)
    yield lr
    listener.stop()
    lr.stop()


def _make_exec_control(exec_port: int, instance_id: str = "c1") -> ControlServer:
    registry = Registry()
    registry.register(ClientEntry(
        pid=1, instance_id=instance_id, instance_name="test",
        exec_host="localhost", exec_port=exec_port, alias=None,
    ))
    return ControlServer(registry)


def test_exec_python_success(
    listener_runner, listener_port: int, monkeypatch,
) -> None:
    """exec_python returns status and stdout for a valid target."""
    control = _make_exec_control(listener_port)
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: control)
    workflow_id = WorkflowPersistence.create_workflow("test_wf")

    content, _raw = asyncio.run(shim_mcp.call_tool("exec_python", {
        "instance_id": "c1",
        "code": 'print("hello")',
        "workflow_id": workflow_id,
    }))
    data = json.loads(content[0].text)
    assert "status" in data
    assert "stdout" in data


def test_exec_python_target_not_found(
    app_runner, monkeypatch,
) -> None:
    """exec_python raises ToolError when instance_id is unknown."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)

    with pytest.raises(ToolError):
        asyncio.run(shim_mcp.call_tool("exec_python", {
            "instance_id": "nonexistent",
            "code": "print(1)",
            "workflow_id": "dummy",
        }))


# ---------------------------------------------------------------------------
# E4 — start_workflow tool
# ---------------------------------------------------------------------------

def test_start_workflow_returns_workflow_id(
    app_runner, monkeypatch,
) -> None:
    """start_workflow returns a non-empty workflow_id."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)

    content, _raw = asyncio.run(shim_mcp.call_tool("start_workflow", {"name": "my-wf"}))
    data = json.loads(content[0].text)
    assert "workflow_id" in data
    assert data["workflow_id"]


def test_start_workflow_creates_file(
    app_runner, monkeypatch,
) -> None:
    """start_workflow creates the corresponding JSON file on disk."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)

    content, _raw = asyncio.run(shim_mcp.call_tool("start_workflow", {"name": "disk-test"}))
    workflow_id = json.loads(content[0].text)["workflow_id"]
    assert WorkflowPersistence.exists(workflow_id)
