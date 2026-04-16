"""Multi-shim integration test: two BackendClients share one backend."""
from __future__ import annotations

import asyncio
import threading
from typing import Iterator

import pytest

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.exec_listener import ExecListener
from python_bridge_mcp.server.backend_client import BackendClient
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import Registry
from python_bridge_mcp.shared.discovery_models import RegisterDiscovery
from python_bridge_mcp.shared.exec_models import ExecStatus
from python_bridge_mcp.shared.jsonline import AsyncJsonLineCodec
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence

import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from conftest import AsyncRunner, free_port  # noqa: E402


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


@pytest.fixture
def listener_runner(exec_port: int) -> Iterator[AsyncRunner]:
    executor = CodeExecutor()
    runner_obj = DirectRunner(executor)
    listener = ExecListener("localhost", exec_port, runner_obj)
    runner = AsyncRunner()
    runner.start(listener.run)
    yield runner
    listener.stop()
    runner.stop()


def _bg_register(discovery_port: int, exec_port: int, instance_id: str = "c1") -> None:
    done = threading.Event()

    def _run() -> None:
        async def _do() -> None:
            reader, writer = await asyncio.open_connection("localhost", discovery_port)
            try:
                msg = RegisterDiscovery(
                    pid=1, instance_id=instance_id, instance_name="test",
                    exec_host="localhost", exec_port=exec_port,
                )
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                done.set()
                await asyncio.sleep(10)
            finally:
                writer.close()
        asyncio.run(_do())

    threading.Thread(target=_run, daemon=True).start()
    assert done.wait(timeout=3), "registration failed"


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_both_clients_see_same_dcc(
    backend, client_a, client_b, discovery_port: int, exec_port: int,
) -> None:
    """Two BackendClients connected to the same backend both see the registered DCC."""
    _bg_register(discovery_port, exec_port, instance_id="dcc1")

    targets_a = asyncio.run(client_a.list_targets())
    targets_b = asyncio.run(client_b.list_targets())

    assert len(targets_a.targets) == 1
    assert len(targets_b.targets) == 1
    assert targets_a.targets[0].instance_id == targets_b.targets[0].instance_id == "dcc1"


def test_execution_visible_to_second_client(
    backend, client_a, client_b,
    listener_runner, discovery_port: int, exec_port: int,
) -> None:
    """Client A executes code; Client B can see the execution in the workflow overview."""
    _bg_register(discovery_port, exec_port, instance_id="dcc1")

    wf_id = asyncio.run(client_a.start_workflow("shared-wf"))

    result = asyncio.run(client_a.execute("dcc1", 'print("hi")', wf_id))
    assert result.status == ExecStatus.SUCCEEDED

    overview = asyncio.run(client_b.get_workflow_overview(wf_id))
    assert overview.execution_count == 1
    assert len(overview.target_summaries) == 1
    assert overview.target_summaries[0].instance_id == "dcc1"


def test_workflow_created_by_a_readable_by_b(
    backend, client_a, client_b,
) -> None:
    """A workflow created by client A is immediately visible to client B."""
    wf_id = asyncio.run(client_a.start_workflow("cross-client"))
    overview = asyncio.run(client_b.get_workflow_overview(wf_id))
    assert overview.workflow_id == wf_id
    assert overview.name == "cross-client"


def test_alias_set_by_a_visible_via_b(
    backend, client_a, client_b, discovery_port: int, exec_port: int,
) -> None:
    """Alias set via client A is reflected when client B lists targets."""
    _bg_register(discovery_port, exec_port, instance_id="dcc1")

    asyncio.run(client_a.set_alias("dcc1", "my-maya"))

    targets = asyncio.run(client_b.list_targets())
    assert targets.targets[0].alias == "my-maya"
