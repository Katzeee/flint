"""Tests for the BackendClient TCP layer against an in-process backend."""
from __future__ import annotations

import asyncio
import threading
import time
from typing import Iterator

import pytest

from python_bridge_mcp.server.backend_client import BackendClient
from python_bridge_mcp.server.control_models import PingRequest
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import ClientEntry, Registry
from python_bridge_mcp.shared.workflow_persistence import (
    WorkflowPersistence,
    WorkflowRecordUnavailableError,
)

from conftest import AsyncRunner, free_port


@pytest.fixture
def discovery_port() -> int:
    return free_port()


@pytest.fixture
def api_port() -> int:
    return free_port()


@pytest.fixture
def backend(discovery_port: int, api_port: int) -> Iterator[tuple]:
    registry = Registry(host="localhost", port=discovery_port)
    control = ControlServer(registry, host="localhost", port=api_port)
    runner = AsyncRunner()
    runner.start(lambda: asyncio.gather(registry.run(), control.run()))
    yield registry, control
    runner.stop()


@pytest.fixture
def client(api_port: int) -> BackendClient:
    return BackendClient(host="localhost", port=api_port)


def test_list_instances_empty(backend, client) -> None:
    result = asyncio.run(client.list_instances())
    assert result.instances == []


def test_list_instances_with_registered_instance(
    backend, client, discovery_port: int,
) -> None:
    registry, _ = backend
    registry.register(ClientEntry(
        pid=1, instance_id="c1", instance_name="test", instance_type="maya",
    ))
    result = asyncio.run(client.list_instances())
    assert len(result.instances) == 1
    assert result.instances[0].instance_id == "c1"


def test_list_instances_filter_by_type(backend, client, discovery_port: int) -> None:
    registry, _ = backend
    registry.register(ClientEntry(pid=1, instance_id="maya1", instance_name="maya1", instance_type="maya"))
    registry.register(ClientEntry(pid=2, instance_id="nuke1", instance_name="nuke1", instance_type="nuke"))

    result = asyncio.run(client.list_instances(instance_type="maya"))
    assert len(result.instances) == 1
    assert result.instances[0].instance_id == "maya1"


def test_execute_unknown_instance_raises(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf"))
    with pytest.raises(KeyError):
        asyncio.run(client.execute("nonexistent", "x=1", wf_id))


def test_get_workflow_execution_not_found(backend, client) -> None:
    wf_id = asyncio.run(client.start_workflow("wf"))
    with pytest.raises(KeyError):
        asyncio.run(client.get_workflow_execution(wf_id, "9999"))


def test_backend_client_ping(backend, client) -> None:
    assert asyncio.run(client.ping()) is True


def test_ping_reports_not_ready_until_backend_is_fully_started() -> None:
    registry = Registry()
    control = ControlServer(registry, ready=False)

    response = asyncio.run(control._dispatch(PingRequest()))
    assert response.ready is False

    control.set_ready(True)
    response = asyncio.run(control._dispatch(PingRequest()))
    assert response.ready is True


def test_backend_error_propagated(backend, client) -> None:
    with pytest.raises(WorkflowRecordUnavailableError):
        asyncio.run(client.get_workflow_execution("does-not-exist", "0001"))
