import asyncio
import json
import threading
from datetime import datetime, timezone
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
from python_bridge_mcp.shared.exec_models import ExecStatus
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
    """exec_python returns ExecResult with TARGET_OFFLINE error for unknown instance_id."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)

    content, _raw = asyncio.run(shim_mcp.call_tool("exec_python", {
        "instance_id": "nonexistent",
        "code": "print(1)",
        "workflow_id": "dummy",
    }))
    data = json.loads(content[0].text)
    assert data["status"] == "failed"
    assert data["error"] == "target_offline"


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


# ---------------------------------------------------------------------------
# E5 — get_workflow_overview tool
# ---------------------------------------------------------------------------

def test_get_workflow_overview_existing(app_runner, monkeypatch) -> None:
    """get_workflow_overview returns summary for an existing workflow."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    workflow_id = WorkflowPersistence.create_workflow("my-wf", "desc")

    content, _raw = asyncio.run(shim_mcp.call_tool("get_workflow_overview", {
        "workflow_id": workflow_id,
    }))
    data = json.loads(content[0].text)
    assert data["workflow_id"] == workflow_id
    assert data["name"] == "my-wf"


def test_get_workflow_overview_not_found(app_runner, monkeypatch) -> None:
    """get_workflow_overview raises ToolError for unknown workflow_id."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)

    with pytest.raises(ToolError):
        asyncio.run(shim_mcp.call_tool("get_workflow_overview", {
            "workflow_id": "nonexistent",
        }))


# ---------------------------------------------------------------------------
# E6 — get_workflow_execution tool
# ---------------------------------------------------------------------------

def _setup_workflow_with_execution(control: ControlServer) -> tuple:
    """Create a workflow and append a completed execution. Returns (workflow_id, execution_id)."""
    workflow_id = WorkflowPersistence.create_workflow("wf")
    execution_id = WorkflowPersistence.append_running_execution(workflow_id, "run1", "c1", "print(1)")
    WorkflowPersistence.update_execution_result(
        workflow_id, execution_id, ExecStatus.SUCCEEDED,
        "1\n", "", datetime.now(timezone.utc).isoformat(),
    )
    return workflow_id, execution_id


def test_get_workflow_execution_full_view(app_runner, monkeypatch) -> None:
    """view='full' includes the code field."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    workflow_id, execution_id = _setup_workflow_with_execution(app.control)

    content, _raw = asyncio.run(shim_mcp.call_tool("get_workflow_execution", {
        "workflow_id": workflow_id,
        "execution_id": execution_id,
        "view": "full",
    }))
    data = json.loads(content[0].text)
    assert "code" in data
    assert data["code"] == "print(1)"


def test_get_workflow_execution_summary_no_code(app_runner, monkeypatch) -> None:
    """view='summary' omits the code field."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    workflow_id, execution_id = _setup_workflow_with_execution(app.control)

    content, _raw = asyncio.run(shim_mcp.call_tool("get_workflow_execution", {
        "workflow_id": workflow_id,
        "execution_id": execution_id,
        "view": "summary",
    }))
    data = json.loads(content[0].text)
    assert "code" not in data


def test_get_workflow_execution_not_found(app_runner, monkeypatch) -> None:
    """get_workflow_execution raises ToolError for unknown execution_id."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    workflow_id = WorkflowPersistence.create_workflow("wf")

    with pytest.raises(ToolError):
        asyncio.run(shim_mcp.call_tool("get_workflow_execution", {
            "workflow_id": workflow_id,
            "execution_id": "9999",
        }))


# ---------------------------------------------------------------------------
# E7 — set_target_alias tool
# ---------------------------------------------------------------------------

def test_set_target_alias_updates_list(
    app_runner, discovery_port: int, exec_port: int, monkeypatch,
) -> None:
    """After set_target_alias, list_dcc_targets returns the updated alias."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    _bg_register(discovery_port, exec_port, instance_id="c1")

    asyncio.run(shim_mcp.call_tool("set_target_alias", {
        "instance_id": "c1",
        "alias": "my-alias",
    }))

    content, _raw = asyncio.run(shim_mcp.call_tool("list_dcc_targets", {}))
    data = json.loads(content[0].text)
    assert data[0]["alias"] == "my-alias"


def test_set_target_alias_empty_string_becomes_none(
    app_runner, discovery_port: int, exec_port: int, monkeypatch,
) -> None:
    """set_target_alias with empty string returns alias=None."""
    app, _ = app_runner
    monkeypatch.setattr("python_bridge_mcp.server.shim._get_control", lambda: app.control)
    _bg_register(discovery_port, exec_port, instance_id="c1")

    content, _raw = asyncio.run(shim_mcp.call_tool("set_target_alias", {
        "instance_id": "c1",
        "alias": "",
    }))
    data = json.loads(content[0].text)
    assert data["alias"] is None
