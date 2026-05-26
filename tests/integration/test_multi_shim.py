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


class _InstanceRunner:
    def __init__(self, client: DiscoveryClient) -> None:
        self.client = client
        self._thread = threading.Thread(target=client.run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self.client.stop()
        self._thread.join(timeout=5)


def _start_instance(
    discovery_port: int,
    instance_id: str = "c1",
    alias: Optional[str] = None,
) -> _InstanceRunner:
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
    runner = _InstanceRunner(client)
    runner.start()
    assert client.wait_until_registered(timeout=3), "registration failed"
    return runner


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_both_clients_see_same_instance(
    backend, client_a, client_b, discovery_port: int,
) -> None:
    """Two BackendClients connected to the same backend both see the registered instance."""
    instance = _start_instance(discovery_port, instance_id="instance1")
    try:
        instances_a = asyncio.run(client_a.list_instances())
        instances_b = asyncio.run(client_b.list_instances())

        assert len(instances_a.instances) == 1
        assert len(instances_b.instances) == 1
        assert instances_a.instances[0].instance_id == instances_b.instances[0].instance_id == "instance1"
    finally:
        instance.stop()


