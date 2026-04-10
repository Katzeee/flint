import asyncio
import socket
import threading
from typing import Iterator

import pytest

from pbridge.backend.app import App
from pbridge.backend.control_server import ControlServer
from pbridge.client.code_executor import CodeExecutor
from pbridge.client.code_runner import DirectRunner
from pbridge.client.exec_listener import ExecListener
from pbridge.shared.exec_models import ExecStatus


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("", 0))
        return s.getsockname()[1]


class _AsyncRunner:
    """Runs async servers in a background thread with its own event loop."""

    def __init__(self) -> None:
        self._loop: asyncio.AbstractEventLoop
        self._thread: threading.Thread

    def start(self, coro_fn) -> None:
        ready = threading.Event()

        def _thread_target() -> None:
            self._loop = asyncio.new_event_loop()
            asyncio.set_event_loop(self._loop)
            try:
                self._loop.run_until_complete(self._boot(coro_fn, ready))
            except (asyncio.CancelledError, RuntimeError):
                pass
            finally:
                pending = asyncio.all_tasks(self._loop)
                if pending:
                    for t in pending:
                        t.cancel()
                    self._loop.run_until_complete(
                        asyncio.gather(*pending, return_exceptions=True)
                    )
                self._loop.close()

        self._thread = threading.Thread(target=_thread_target, daemon=True)
        self._thread.start()
        assert ready.wait(timeout=5), "server did not start in time"

    async def _boot(self, coro_fn, ready: threading.Event) -> None:
        task = asyncio.ensure_future(coro_fn())
        await asyncio.sleep(0.05)
        ready.set()
        await task

    def stop(self) -> None:
        self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(timeout=5)

    def run_async(self, coro):
        future = asyncio.run_coroutine_threadsafe(coro, self._loop)
        return future.result(timeout=10)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture
def discovery_port() -> int:
    return _free_port()


@pytest.fixture
def exec_port() -> int:
    return _free_port()


@pytest.fixture
def app_runner(discovery_port: int) -> Iterator[tuple]:
    app = App(discovery_host="localhost", discovery_port=discovery_port)
    runner = _AsyncRunner()
    runner.start(app.run)
    yield app, runner
    app.stop()
    runner.stop()


@pytest.fixture
def listener_runner(exec_port: int) -> Iterator[_AsyncRunner]:
    executor = CodeExecutor()
    code_runner = DirectRunner(executor)
    listener = ExecListener("localhost", exec_port, code_runner)
    runner = _AsyncRunner()
    runner.start(listener.run)
    yield runner
    listener.stop()
    runner.stop()


def _register_client(
    discovery_port: int, exec_host: str, exec_port: int, instance_id: str = "test-1",
) -> None:
    """Register a fake client with the discovery server."""
    from pbridge.shared.discovery_models import RegisterDiscovery, AckDiscovery
    from pbridge.shared.jsonline import AsyncJsonLineCodec
    from pbridge.shared.model_base import VersionedWireModel

    async def _do_register():
        reader, writer = await asyncio.open_connection("localhost", discovery_port)
        try:
            msg = RegisterDiscovery(
                pid="1", instance_id=instance_id, instance_name="test",
                exec_host=exec_host, exec_port=exec_port,
            )
            await AsyncJsonLineCodec.send(writer, msg.to_dict())
            data = await AsyncJsonLineCodec.recv(reader)
            ack = VersionedWireModel.parse_versioned(data)
            assert isinstance(ack, AckDiscovery) and ack.success
            # Keep connection alive briefly so entry stays registered
            await asyncio.sleep(0.5)
        finally:
            writer.close()

    asyncio.run(_do_register())


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_app_starts(app_runner, discovery_port: int) -> None:
    """App starts and discovery server is listening."""
    app, runner = app_runner
    # Verify we can connect to the discovery port
    import socket
    with socket.create_connection(("localhost", discovery_port), timeout=2):
        pass


def test_list_clients_empty(app_runner) -> None:
    app, runner = app_runner
    clients = app.control.list_clients()
    assert clients == {}


def test_execute_unknown_client(app_runner) -> None:
    app, runner = app_runner
    with pytest.raises(KeyError, match="unknown client"):
        asyncio.run(app.control.execute("nonexistent", "print(1)"))


def test_execute_on_client(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    """Register a client, then execute code on it via ControlServer."""
    app, app_run = app_runner

    # Register client in a background thread so the connection stays alive
    register_done = threading.Event()
    register_error = [None]

    def _register():
        from pbridge.shared.discovery_models import RegisterDiscovery
        from pbridge.shared.jsonline import AsyncJsonLineCodec
        from pbridge.shared.model_base import VersionedWireModel

        async def _do():
            reader, writer = await asyncio.open_connection("localhost", discovery_port)
            try:
                msg = RegisterDiscovery(
                    pid="1", instance_id="c1", instance_name="test",
                    exec_host="localhost", exec_port=exec_port,
                )
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                register_done.set()
                # Keep alive while test runs
                await asyncio.sleep(5)
            except Exception as e:
                register_error[0] = e
                register_done.set()
            finally:
                writer.close()

        asyncio.run(_do())

    t = threading.Thread(target=_register, daemon=True)
    t.start()
    assert register_done.wait(timeout=3), "registration failed"
    assert register_error[0] is None

    # Execute via control server
    result = app_run.run_async(app.control.execute("c1", 'print("hello")'))
    assert result.status == ExecStatus.SUCCEED
    assert result.stdout == "hello\n"


def test_list_clients_after_register(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner

    register_done = threading.Event()

    def _register():
        from pbridge.shared.discovery_models import RegisterDiscovery
        from pbridge.shared.jsonline import AsyncJsonLineCodec

        async def _do():
            reader, writer = await asyncio.open_connection("localhost", discovery_port)
            try:
                msg = RegisterDiscovery(
                    pid="1", instance_id="c1", instance_name="myapp",
                    exec_host="localhost", exec_port=exec_port,
                )
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                register_done.set()
                await asyncio.sleep(5)
            finally:
                writer.close()

        asyncio.run(_do())

    t = threading.Thread(target=_register, daemon=True)
    t.start()
    assert register_done.wait(timeout=3)

    clients = app.control.list_clients()
    assert "c1" in clients
    assert clients["c1"].instance_name == "myapp"
    assert clients["c1"].exec_host == "localhost"
    assert clients["c1"].exec_port == exec_port
