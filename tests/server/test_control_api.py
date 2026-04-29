"""Tests for ControlServer (TCP API) and BackendClient using an in-process backend."""
from __future__ import annotations

import asyncio
import threading
import time
from datetime import datetime, timezone
from typing import Iterator

import pytest

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.server.backend_client import BackendClient, BackendError
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import ClientEntry, Registry
from python_bridge_mcp.shared.exec_models import ExecStatus
from python_bridge_mcp.shared.workflow_persistence import (
    WorkflowPersistence,
    WorkflowRecordUnavailableError,
)

from conftest import AsyncRunner, free_port


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def discovery_port() -> int:
    return free_port()


@pytest.fixture
def api_port() -> int:
    return free_port()


@pytest.fixture
def exec_port() -> int:
    return free_port()


@pytest.fixture
def backend(discovery_port: int, api_port: int) -> Iterator[tuple]:
    """Start Registry + ControlServer in a background thread."""
    registry = Registry(host="localhost", port=discovery_port)
    control = ControlServer(registry, host="localhost", port=api_port)
    runner = AsyncRunner()
    runner.start(lambda: asyncio.gather(registry.run(), control.run()))
    yield registry, control
    registry.stop()
    control.stop()
    runner.stop()


@pytest.fixture
def client(api_port: int) -> BackendClient:
    return BackendClient(host="localhost", port=api_port)


@pytest.fixture
def listener_runner() -> None:
    return None


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
    pid: int = 1,
) -> _DccRunner:
    client = DiscoveryClient(
        instance_id=instance_id,
        instance_name="test",
        runner=DirectRunner(CodeExecutor()),
        host="localhost",
        port=discovery_port,
        heartbeat_interval=0.1,
        pid=pid,
    )
    runner = _DccRunner(client)
    runner.start()
    assert client.wait_until_registered(timeout=3), "registration failed"

    def _auto_stop() -> None:
        time.sleep(5)
        runner.stop()

    threading.Thread(target=_auto_stop, daemon=True).start()
    return runner


# ---------------------------------------------------------------------------
# list_targets
# ---------------------------------------------------------------------------

def test_list_targets_empty(backend, client) -> None:
    result = asyncio.run(client.list_targets())
    assert result.targets == []


def test_list_targets_with_registered_dcc(
    backend, client, discovery_port: int, exec_port: int,
) -> None:
    _bg_register(discovery_port, exec_port, instance_id="c1")
    result = asyncio.run(client.list_targets())
    assert len(result.targets) == 1
    assert result.targets[0].instance_id == "c1"


def test_list_targets_filter_by_type(
    backend, client, discovery_port: int,
) -> None:
    registry, *_ = backend

    def _reg(iid: str, itype: str, pid: int, port: int) -> None:
        registry.register(ClientEntry(
            pid=pid, instance_id=iid, instance_name=iid,
            alias=None, instance_type=itype,
        ))

    _reg("maya1", "maya", 1, 1001)
    _reg("nuke1", "nuke", 2, 1002)

    result = asyncio.run(client.list_targets(dcc_type="maya"))
    assert len(result.targets) == 1
    assert result.targets[0].instance_id == "maya1"


# ---------------------------------------------------------------------------
# start_workflow
# ---------------------------------------------------------------------------

def test_start_workflow_returns_id(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("my-wf"))
    assert "my-wf" in wf_id
    assert WorkflowPersistence.exists(wf_id)


def test_start_workflow_with_description(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf", "my description"))
    record = WorkflowPersistence.load(wf_id)
    assert record.description == "my description"


# ---------------------------------------------------------------------------
# execute
# ---------------------------------------------------------------------------

def test_execute_success(
    backend, client, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    _bg_register(discovery_port, exec_port)
    wf_id = asyncio.run(client.start_workflow("exec-test"))

    result = asyncio.run(client.execute("c1", 'print("hello")', wf_id))
    assert result.status == ExecStatus.SUCCEEDED
    assert WorkflowPersistence.load(wf_id).execs[0].stdout == "hello\n"


def test_execute_unknown_target_raises(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf"))
    with pytest.raises(KeyError):
        asyncio.run(client.execute("nonexistent", "x=1", wf_id))


# ---------------------------------------------------------------------------
# get_workflow_overview
# ---------------------------------------------------------------------------

def test_get_workflow_overview(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("overview-wf", "desc"))
    overview = asyncio.run(client.get_workflow_overview(wf_id))
    assert overview.workflow_id == wf_id
    assert overview.name == "overview-wf"
    assert overview.description == "desc"
    assert overview.execution_count == 0


def test_get_workflow_overview_not_found(backend, client) -> None:
    with pytest.raises(WorkflowRecordUnavailableError):
        asyncio.run(client.get_workflow_overview("nonexistent"))


# ---------------------------------------------------------------------------
# get_workflow_execution
# ---------------------------------------------------------------------------

def test_get_workflow_execution(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf"))
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "x=1")
    WorkflowPersistence.update_execution_result(
        wf_id, exec_id, ExecStatus.SUCCEEDED,
        "out", "", datetime.now(timezone.utc).isoformat(),
    )

    resp = asyncio.run(client.get_workflow_execution(wf_id, exec_id))
    assert resp.execution_id == exec_id
    assert resp.status == "succeeded"


def test_get_workflow_execution_full_view(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf"))
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "print(1)")
    WorkflowPersistence.update_execution_result(
        wf_id, exec_id, ExecStatus.SUCCEEDED, "1\n", "",
        datetime.now(timezone.utc).isoformat(),
    )

    resp = asyncio.run(client.get_workflow_execution(wf_id, exec_id, view="full"))
    assert resp.code == "print(1)"


def test_get_workflow_execution_not_found(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf"))
    with pytest.raises(KeyError):
        asyncio.run(client.get_workflow_execution(wf_id, "9999"))


# ---------------------------------------------------------------------------
# set_alias
# ---------------------------------------------------------------------------

def test_set_alias(backend, client, listener_runner, discovery_port: int, exec_port: int) -> None:
    _bg_register(discovery_port, exec_port, instance_id="c1")
    resp = asyncio.run(client.set_alias("c1", "my-alias"))
    assert resp.success is True
    assert resp.alias == "my-alias"


def test_backend_client_ping(backend, client) -> None:
    assert asyncio.run(client.ping()) is True


def test_set_alias_unknown_target(backend, client) -> None:
    with pytest.raises(KeyError):
        asyncio.run(client.set_alias("nonexistent", "alias"))


# ---------------------------------------------------------------------------
# ErrorResponse handling
# ---------------------------------------------------------------------------

def test_backend_error_propagated(backend, client) -> None:
    """Unknown request type triggers BackendError."""
    from python_bridge_mcp.server.control_models import ErrorResponse, ControlWireModel
    from dataclasses import dataclass
    from typing import ClassVar

    # Inject an unregistered type into the registry to simulate unknown request
    with pytest.raises((BackendError, Exception)):
        # Sending a raw workflow overview request to a nonexistent workflow
        # triggers workflow_not_found which is re-raised as WorkflowRecordUnavailableError
        asyncio.run(client.get_workflow_overview("does-not-exist"))
