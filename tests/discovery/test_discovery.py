from __future__ import annotations

import threading
from typing import Iterator

import pytest

from pbridge.client.discovery import DiscoveryClient, DiscoveryState
from pbridge.server.registry import Registry
from pbridge.shared.discovery_models import RegisterDiscovery

from conftest import AsyncRunner, free_port, wait_for

HEARTBEAT = 0.3  # short interval for fast tests


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _wait_connected(client: DiscoveryClient, timeout: float = 3.0) -> bool:
    return client._connected_event.wait(timeout)


class _ClientRunner:
    def __init__(self, client: DiscoveryClient) -> None:
        self.client = client
        self._thread: threading.Thread

    def start(self) -> None:
        self._thread = threading.Thread(target=self.client.run, daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self.client.stop()
        self._thread.join(timeout=5)


def _client(port: int, instance_id: str, instance_name: str = "Test Client") -> DiscoveryClient:
    return DiscoveryClient(
        instance_id=instance_id,
        instance_name=instance_name,
        exec_host="localhost",
        exec_port=9000,
        host="localhost",
        port=port,
        heartbeat_interval=HEARTBEAT,
    )


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def port() -> int:
    return free_port()


@pytest.fixture
def srv(port: int) -> Iterator[tuple]:
    server = Registry(host="localhost", port=port)
    runner = AsyncRunner()
    runner.start(server.run)
    yield server, runner
    server.stop()
    runner.stop()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_client_connects_and_is_registered(srv, port: int) -> None:
    server, srv_runner = srv
    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    try:
        assert _wait_connected(c), "client did not connect"
        assert c.state == DiscoveryState.CONNECTED
        clients = server.list_clients()
        entry = clients["c1"]
        assert entry.instance_name == "Test Client"
        assert entry.exec_host == "localhost"
        assert entry.exec_port == 9000
    finally:
        r.stop()


def test_client_stop_transitions_to_stopped(srv, port: int) -> None:
    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    assert _wait_connected(c)

    r.stop()

    assert c.state == DiscoveryState.STOPPED


def test_client_disconnect_removes_entry_from_server(srv, port: int) -> None:
    server, srv_runner = srv
    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    assert _wait_connected(c)
    assert "c1" in server.list_clients()

    r.stop()

    assert wait_for(lambda: "c1" not in server.list_clients()), \
        "server did not remove client entry after disconnect"


def test_server_disconnect_moves_client_to_connecting(port: int) -> None:
    server = Registry(host="localhost", port=port)
    runner = AsyncRunner()
    runner.start(server.run)

    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    try:
        assert _wait_connected(c)

        runner.stop()

        assert wait_for(lambda: c.state == DiscoveryState.CONNECTING), \
            "client did not fall back to CONNECTING after server stopped"
    finally:
        r.stop()


def test_client_reconnects_after_server_restart(port: int) -> None:
    server1 = Registry(host="localhost", port=port)
    runner1 = AsyncRunner()
    runner1.start(server1.run)

    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    try:
        assert _wait_connected(c)

        runner1.stop()
        assert wait_for(lambda: c.state == DiscoveryState.CONNECTING)

        server2 = Registry(host="localhost", port=port)
        runner2 = AsyncRunner()
        runner2.start(server2.run)
        try:
            assert _wait_connected(c, timeout=5), "client did not reconnect after server restart"
            assert c.state == DiscoveryState.CONNECTED
            assert "c1" in server2.list_clients()
        finally:
            runner2.stop()
    finally:
        r.stop()


def test_multiple_clients_all_registered(srv, port: int) -> None:
    server, srv_runner = srv
    ids = ["c1", "c2", "c3"]
    runners = [_ClientRunner(_client(port, cid)) for cid in ids]
    for r in runners:
        r.start()
    try:
        for r in runners:
            assert _wait_connected(r.client), f"{r.client._instance_id} did not connect"
        registered = server.list_clients()
        for cid in ids:
            assert cid in registered
    finally:
        for r in runners:
            r.stop()


def test_server_stop_with_no_clients(port: int) -> None:
    server = Registry(host="localhost", port=port)
    runner = AsyncRunner()
    runner.start(server.run)
    server.stop()
    runner.stop()


def test_register_discovery_pid_is_int() -> None:
    msg = RegisterDiscovery(
        pid=1234, instance_id="c1", instance_name="test",
        exec_host="localhost", exec_port=9000,
    )
    data = msg.to_dict()
    assert isinstance(data["pid"], int)
    assert data["pid"] == 1234


def test_client_entry_pid_equality() -> None:
    from pbridge.server.registry import ClientEntry
    import time
    e1 = ClientEntry(pid=42, instance_id="c1", instance_name="t", exec_host="h", exec_port=1, alias=None)
    e2 = ClientEntry(pid=42, instance_id="c2", instance_name="t", exec_host="h", exec_port=1, alias=None)
    assert e1.pid == e2.pid


def test_client_state_sequence(srv, port: int) -> None:
    c = _client(port, "c1")
    r = _ClientRunner(c)

    assert c.state == DiscoveryState.STOPPED

    r.start()
    assert _wait_connected(c)
    assert c.state == DiscoveryState.CONNECTED

    r.stop()
    assert c.state == DiscoveryState.STOPPED
