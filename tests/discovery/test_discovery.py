from __future__ import annotations

import asyncio
import socket
import threading
import time
from typing import Iterator

import pytest

from pbridge.client.discovery import DiscoveryClient, DiscoveryState
from pbridge.server.discovery import DiscoveryServer

HEARTBEAT = 0.3  # short interval for fast tests


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


def _wait_connected(client: DiscoveryClient, timeout: float = 3.0) -> bool:
    return client._connected_event.wait(timeout)


def _wait_for(condition, timeout: float = 3.0, poll: float = 0.05) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if condition():
            return True
        time.sleep(poll)
    return False


class _ServerRunner:
    def __init__(self, host: str, port: int) -> None:
        self.server = DiscoveryServer(host=host, port=port)
        self._loop: asyncio.AbstractEventLoop
        self._thread: threading.Thread

    def start(self) -> None:
        ready = threading.Event()

        def _thread_target() -> None:
            self._loop = asyncio.new_event_loop()
            asyncio.set_event_loop(self._loop)
            try:
                self._loop.run_until_complete(self._boot(ready))
            except (asyncio.CancelledError, RuntimeError):
                pass
            finally:
                # cancel remaining tasks (active connections) so writers are closed
                pending = asyncio.all_tasks(self._loop)
                if pending:
                    for t in pending:
                        t.cancel()
                    self._loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
                self._loop.close()

        self._thread = threading.Thread(target=_thread_target, daemon=True)
        self._thread.start()
        assert ready.wait(timeout=5), "server did not start in time"

    async def _boot(self, ready: threading.Event) -> None:
        task = asyncio.ensure_future(self.server.run())
        await asyncio.sleep(0.05)  # allow start_server to bind
        ready.set()
        await task

    def stop(self) -> None:
        # stop() only closes the listener; call loop.stop() to also tear down
        # active connection handlers so clients detect the disconnect
        self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(timeout=5)


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
    return _free_port()


@pytest.fixture
def srv(port: int) -> Iterator[_ServerRunner]:
    runner = _ServerRunner("localhost", port)
    runner.start()
    yield runner
    runner.stop()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_client_connects_and_is_registered(srv: _ServerRunner, port: int) -> None:
    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    try:
        assert _wait_connected(c), "client did not connect"
        assert c.state == DiscoveryState.CONNECTED
        entry = srv.server.clients["c1"]
        assert entry.instance_name == "Test Client"
        assert entry.exec_host == "localhost"
        assert entry.exec_port == 9000
    finally:
        r.stop()


def test_client_stop_transitions_to_stopped(srv: _ServerRunner, port: int) -> None:
    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    assert _wait_connected(c)

    r.stop()

    assert c.state == DiscoveryState.STOPPED


def test_client_disconnect_removes_entry_from_server(srv: _ServerRunner, port: int) -> None:
    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    assert _wait_connected(c)
    assert "c1" in srv.server.clients

    r.stop()

    assert _wait_for(lambda: "c1" not in srv.server.clients), \
        "server did not remove client entry after disconnect"


def test_server_disconnect_moves_client_to_connecting(port: int) -> None:
    srv = _ServerRunner("localhost", port)
    srv.start()

    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    try:
        assert _wait_connected(c)

        srv.stop()

        assert _wait_for(lambda: c.state == DiscoveryState.CONNECTING), \
            "client did not fall back to CONNECTING after server stopped"
    finally:
        r.stop()


def test_client_reconnects_after_server_restart(port: int) -> None:
    srv1 = _ServerRunner("localhost", port)
    srv1.start()

    c = _client(port, "c1")
    r = _ClientRunner(c)
    r.start()
    try:
        assert _wait_connected(c)

        srv1.stop()
        assert _wait_for(lambda: c.state == DiscoveryState.CONNECTING)

        srv2 = _ServerRunner("localhost", port)
        srv2.start()
        try:
            assert _wait_connected(c, timeout=5), "client did not reconnect after server restart"
            assert c.state == DiscoveryState.CONNECTED
            assert "c1" in srv2.server.clients
        finally:
            srv2.stop()
    finally:
        r.stop()


def test_multiple_clients_all_registered(srv: _ServerRunner, port: int) -> None:
    ids = ["c1", "c2", "c3"]
    runners = [_ClientRunner(_client(port, cid)) for cid in ids]
    for r in runners:
        r.start()
    try:
        for r in runners:
            assert _wait_connected(r.client), f"{r.client._instance_id} did not connect"
        registered = srv.server.clients
        for cid in ids:
            assert cid in registered
    finally:
        for r in runners:
            r.stop()


def test_server_stop_with_no_clients(port: int) -> None:
    srv = _ServerRunner("localhost", port)
    srv.start()
    srv.stop()  # should not raise


def test_client_state_sequence(srv: _ServerRunner, port: int) -> None:
    c = _client(port, "c1")
    r = _ClientRunner(c)

    assert c.state == DiscoveryState.STOPPED

    r.start()
    assert _wait_connected(c)
    assert c.state == DiscoveryState.CONNECTED

    r.stop()
    assert c.state == DiscoveryState.STOPPED
