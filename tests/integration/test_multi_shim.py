"""Multi-shim integration test: two BackendClients share one backend."""
from __future__ import annotations

import asyncio
import threading
from typing import Iterator, Optional

import pytest

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.server.backend_client import BackendClient
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import Registry
from python_bridge_mcp.shared.exec_models import ExecStatus

import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from conftest import AsyncRunner, free_port, wait_for  # noqa: E402


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
def backend(discovery_port: int, api_port: int) -> Iterator[tuple]:
    registry = Registry(host="localhost", port=discovery_port)
    control = ControlServer(registry, host="localhost", port=api_port)
    runner = AsyncRunner()
    runner.start(lambda: asyncio.gather(registry.run(), control.run()))
    yield registry, control
    registry.stop()
    control.stop()
    runner.stop()


@pytest.fixture
def client_a(api_port: int) -> BackendClient:
    return BackendClient(host="localhost", port=api_port)


@pytest.fixture
def client_b(api_port: int) -> BackendClient:
    return BackendClient(host="localhost", port=api_port)


class _DccRunner:
    def __init__(self, client: DiscoveryClient) -> None:
        self.client = client
        self._thread = threading.Thread(target=client.run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self.client.stop()
        self._thread.join(timeout=5)


def _start_dcc(
    discovery_port: int,
    instance_id: str = "c1",
    alias: Optional[str] = None,
) -> _DccRunner:
    client = DiscoveryClient(
        instance_id=instance_id,
        instance_name="test",
        runner=DirectRunner(CodeExecutor()),
        alias=alias,
        host="localhost",
        port=discovery_port,
        heartbeat_interval=0.1,
        pid=1,
    )
    runner = _DccRunner(client)
    runner.start()
    assert client.wait_until_registered(timeout=3), "registration failed"
    return runner


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_both_clients_see_same_dcc(
    backend, client_a, client_b, discovery_port: int,
) -> None:
    """Two BackendClients connected to the same backend both see the registered DCC."""
    dcc = _start_dcc(discovery_port, instance_id="dcc1")
    try:
        targets_a = asyncio.run(client_a.list_targets())
        targets_b = asyncio.run(client_b.list_targets())

        assert len(targets_a.targets) == 1
        assert len(targets_b.targets) == 1
        assert targets_a.targets[0].instance_id == targets_b.targets[0].instance_id == "dcc1"
    finally:
        dcc.stop()


def test_execution_visible_to_second_client(
    backend, client_a, client_b,
    discovery_port: int,
) -> None:
    """Client A executes code; Client B can see the execution in the workflow overview."""
    dcc = _start_dcc(discovery_port, instance_id="dcc1")
    try:
        wf_id = asyncio.run(client_a.start_workflow("shared-wf"))

        result = asyncio.run(client_a.execute("dcc1", 'print("hi")', wf_id))
        assert result.status == ExecStatus.SUCCEEDED

        overview = asyncio.run(client_b.get_workflow_overview(wf_id))
        assert overview.execution_count == 1
        assert len(overview.target_summaries) == 1
        assert overview.target_summaries[0].instance_id == "dcc1"
    finally:
        dcc.stop()


def test_workflow_created_by_a_readable_by_b(
    backend, client_a, client_b,
) -> None:
    """A workflow created by client A is immediately visible to client B."""
    wf_id = asyncio.run(client_a.start_workflow("cross-client"))
    overview = asyncio.run(client_b.get_workflow_overview(wf_id))
    assert overview.workflow_id == wf_id
    assert overview.name == "cross-client"


def test_alias_set_by_a_visible_via_b(
    backend, client_a, client_b,
    discovery_port: int,
) -> None:
    """Alias set via client A is reflected when client B lists targets."""
    dcc = _start_dcc(discovery_port, instance_id="dcc1")
    try:
        asyncio.run(client_a.set_alias("dcc1", "my-maya"))

        targets = asyncio.run(client_b.list_targets())
        assert targets.targets[0].alias == "my-maya"
    finally:
        dcc.stop()


def test_alias_survives_re_registration_via_listener_state(
    backend, client_a, client_b,
    discovery_port: int,
) -> None:
    """After re-registration, alias from client state is preserved."""
    dcc = _start_dcc(discovery_port, instance_id="dcc1")
    try:
        asyncio.run(client_a.set_alias("dcc1", "lighting"))
        assert asyncio.run(client_b.list_targets()).targets[0].alias == "lighting"

        alias = dcc.client._current_alias()
    finally:
        dcc.stop()

    assert wait_for(lambda: asyncio.run(client_b.list_targets()).targets == [])

    dcc2 = _start_dcc(discovery_port, instance_id="dcc1", alias=alias)
    try:
        targets = asyncio.run(client_b.list_targets())
        assert len(targets.targets) > 0
        assert targets.targets[0].alias == "lighting"
    finally:
        dcc2.stop()
