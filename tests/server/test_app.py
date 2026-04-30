import asyncio
import threading
import time
from datetime import datetime, timezone
from typing import Iterator, Optional

import pytest

from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import ClientEntry, Registry
from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.shared.instance_control_models import InstanceExecStatus
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence, WorkflowRecordUnavailableError
from python_bridge_mcp.server.control_models import ListTargetsResponse, SetTargetAliasResponse

from conftest import AsyncRunner, free_port, wait_for


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

class _DccRunner:
    def __init__(self, client: DiscoveryClient) -> None:
        self.client = client
        self._thread = threading.Thread(target=client.run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self.client.stop()
        self._thread.join(timeout=5)


def _bg_register(
    discovery_port: int,
    exec_port: int = 0,
    instance_id: str = "c1",
    instance_name: str = "test",
    alias: Optional[str] = None,
    disconnect_event: Optional[threading.Event] = None,
    instance_type: str = "",
    pid: int = 1,
) -> _DccRunner:
    client = DiscoveryClient(
        instance_id=instance_id,
        instance_name=instance_name,
        runner=DirectRunner(CodeExecutor()),
        alias=alias,
        instance_type=instance_type,
        host="localhost",
        port=discovery_port,
        heartbeat_interval=0.1,
        pid=pid,
    )
    runner = _DccRunner(client)
    runner.start()
    assert client.wait_until_registered(timeout=3), "registration failed"

    def _auto_stop() -> None:
        if disconnect_event is not None:
            disconnect_event.wait()
        else:
            time.sleep(5)
        runner.stop()

    threading.Thread(target=_auto_stop, daemon=True).start()
    return runner


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
    registry = Registry(host="localhost", port=discovery_port)
    control = ControlServer(registry)
    runner = AsyncRunner()
    runner.start(registry.run)
    yield registry, control, runner
    registry.stop()
    runner.stop()


@pytest.fixture
def listener_runner() -> None:
    return None


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_registry_list_clients_from_sync_thread(app_runner) -> None:
    registry, control, _ = app_runner
    # Must return a plain dict when called directly from a non-async thread
    result = registry.list_clients()
    assert isinstance(result, dict)


def test_execute_unknown_client(app_runner) -> None:
    registry, control, runner = app_runner
    wf_id = control.start_workflow("test")
    with pytest.raises(KeyError, match="unknown client"):
        runner.run_async(control.execute("nonexistent", "print(1)", wf_id))


def test_execute_on_client(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = control.start_workflow("test")

    result = app_run.run_async(control.execute("c1", 'print("hello")', wf_id))
    assert result.status == InstanceExecStatus.SUCCEEDED
    assert WorkflowPersistence.load(wf_id).execs[0].stdout == "hello\n"


def test_list_clients_after_register(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port, instance_name="myapp")

    clients = registry.list_clients()
    assert "c1" in clients
    assert clients["c1"].instance_name == "myapp"


def test_alias_from_registration(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port, alias="my-alias")

    clients = registry.list_clients()
    assert clients["c1"].alias == "my-alias"


def test_set_alias(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port)

    assert registry.list_clients()["c1"].alias is None
    runner.run_async(control.set_alias("c1", "new-alias"))
    assert registry.list_clients()["c1"].alias == "new-alias"


def test_set_alias_clear(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port, alias="old")

    runner.run_async(control.set_alias("c1", None))
    assert registry.list_clients()["c1"].alias is None


def test_set_alias_unknown_client(app_runner) -> None:
    registry, control, runner = app_runner
    with pytest.raises(KeyError, match="unknown client"):
        runner.run_async(control.set_alias("nonexistent", "alias"))


def test_client_disconnect_removes_entry(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    disconnect = threading.Event()
    _bg_register(discovery_port, exec_port, disconnect_event=disconnect)

    assert "c1" in registry.list_clients()
    disconnect.set()
    assert wait_for(lambda: "c1" not in registry.list_clients()), \
        "client entry not removed after disconnect"


def test_evict_stale_on_register(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner

    stale = ClientEntry(
        pid=0, instance_id="stale-1", instance_name="stale",
        alias=None,
        last_heartbeat=time.monotonic() - 9999,
    )
    registry.register(stale)

    # Register a new client — should evict the stale one
    _bg_register(discovery_port, exec_port)
    clients = registry.list_clients()
    assert "c1" in clients
    assert "stale-1" not in clients


def test_instance_type_stored_on_registration(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port, instance_type="maya")
    clients = registry.list_clients()
    assert clients["c1"].instance_type == "maya"


def test_list_clients_filters_by_instance_type(
    app_runner, discovery_port: int,
) -> None:
    registry, control, app_run = app_runner
    port_maya = free_port()
    port_nuke = free_port()
    _bg_register(discovery_port, port_maya, instance_id="c1", instance_type="maya", pid=1)
    _bg_register(discovery_port, port_nuke, instance_id="c2", instance_type="nuke", pid=2)

    maya_clients = registry.list_clients("maya")
    assert "c1" in maya_clients
    assert "c2" not in maya_clients

    nuke_clients = registry.list_clients("nuke")
    assert "c2" in nuke_clients
    assert "c1" not in nuke_clients

    all_clients = registry.list_clients()
    assert "c1" in all_clients and "c2" in all_clients


def test_same_pid_deduplication(
    app_runner, discovery_port: int,
) -> None:
    registry, control, app_run = app_runner
    port_a = free_port()
    port_b = free_port()
    # Both use pid=99 — same pid, different instance_id
    _bg_register(discovery_port, port_a, instance_id="old", pid=99)
    _bg_register(discovery_port, port_b, instance_id="new", pid=99)
    # "new" registers with the same pid — "old" must be evicted
    clients = registry.list_clients()
    assert "new" in clients
    assert "old" not in clients


def test_set_alias_empty_string_becomes_none(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port, alias="initial")
    runner.run_async(control.set_alias("c1", ""))
    assert registry.list_clients()["c1"].alias is None


def test_set_alias_whitespace_becomes_none(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port, alias="initial")
    runner.run_async(control.set_alias("c1", "  "))
    assert registry.list_clients()["c1"].alias is None


# ---------------------------------------------------------------------------
# D1 — list_targets
# ---------------------------------------------------------------------------

def test_list_targets_returns_target_info(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, _ = app_runner
    _bg_register(discovery_port, exec_port, instance_name="myapp", instance_type="maya")

    response = control.list_targets()
    assert isinstance(response, ListTargetsResponse)
    assert len(response.targets) == 1
    t = response.targets[0]
    assert t.instance_id == "c1"
    assert t.instance_name == "myapp"
    assert t.instance_type == "maya"
    assert t.alias is None


def test_list_targets_filters_by_type(
    app_runner, discovery_port: int,
) -> None:
    registry, control, _ = app_runner
    port_maya = free_port()
    port_nuke = free_port()
    _bg_register(discovery_port, port_maya, instance_id="c1", instance_type="maya", pid=1)
    _bg_register(discovery_port, port_nuke, instance_id="c2", instance_type="nuke", pid=2)

    maya = control.list_targets("maya")
    assert len(maya.targets) == 1
    assert maya.targets[0].instance_id == "c1"

    nuke = control.list_targets("nuke")
    assert len(nuke.targets) == 1
    assert nuke.targets[0].instance_id == "c2"


# ---------------------------------------------------------------------------
# D2 — get_workflow_overview
# ---------------------------------------------------------------------------

def test_get_workflow_overview_existing(app_runner) -> None:
    registry, control, _ = app_runner
    wf_id = control.start_workflow("my-wf", "some description")

    overview = control.get_workflow_overview(wf_id)
    assert overview.workflow_id == wf_id
    assert overview.name == "my-wf"
    assert overview.description == "some description"
    assert overview.execution_count == 0
    assert overview.created_at != ""
    assert overview.instance_ids == []


def test_get_workflow_overview_not_found(app_runner) -> None:
    registry, control, _ = app_runner
    with pytest.raises(WorkflowRecordUnavailableError):
        control.get_workflow_overview("nonexistent-wf-id")


# ---------------------------------------------------------------------------
# D3 — get_workflow_execution
# ---------------------------------------------------------------------------

def test_get_workflow_execution_full_view(app_runner) -> None:
    registry, control, _ = app_runner
    wf_id = control.start_workflow("my-wf")
    execution_id = WorkflowPersistence.append_running_execution(wf_id, "step-1", "c1", "print(42)")
    WorkflowPersistence.update_execution_result(
        wf_id, execution_id, InstanceExecStatus.SUCCEEDED, "42\n", "",
        datetime.now(timezone.utc).isoformat(),
    )

    response = control.get_workflow_execution(wf_id, execution_id, view="full")
    assert response.execution_id == execution_id
    assert response.code == "print(42)"
    assert response.status == InstanceExecStatus.SUCCEEDED.value
    assert response.stdout == "42\n"


def test_get_workflow_execution_summary_view_no_code(app_runner) -> None:
    registry, control, _ = app_runner
    wf_id = control.start_workflow("my-wf")
    execution_id = WorkflowPersistence.append_running_execution(wf_id, "step-1", "c1", "print(42)")

    response = control.get_workflow_execution(wf_id, execution_id, view="summary")
    assert response.code is None


def test_get_workflow_execution_not_found(app_runner) -> None:
    registry, control, _ = app_runner
    wf_id = control.start_workflow("my-wf")

    with pytest.raises(KeyError, match="not found"):
        control.get_workflow_execution(wf_id, "9999", view="summary")


# ---------------------------------------------------------------------------
# D4 — set_alias returns SetTargetAliasResponse
# ---------------------------------------------------------------------------

def test_set_alias_returns_response_with_normalized_alias(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port)

    response = runner.run_async(control.set_alias("c1", ""))
    assert isinstance(response, SetTargetAliasResponse)
    assert response.success is True
    assert response.instance_id == "c1"
    assert response.alias is None


def test_set_alias_returns_response_with_alias_value(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port)

    response = runner.run_async(control.set_alias("c1", "my-alias"))
    assert response.success is True
    assert response.instance_id == "c1"
    assert response.alias == "my-alias"


def test_set_alias_updates_listener_then_registry(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, runner = app_runner
    _bg_register(discovery_port, exec_port, instance_id="c1")

    response = runner.run_async(control.set_alias("c1", "lighting"))
    assert response.success is True
    assert response.alias == "lighting"
    assert registry.list_clients()["c1"].alias == "lighting"
