import asyncio
import threading
import time
from typing import Iterator, Optional

import pytest

from pbridge.server.app import App
from pbridge.server.registry import ClientEntry
from pbridge.client.code_executor import CodeExecutor
from pbridge.client.code_runner import DirectRunner
from pbridge.client.exec_listener import ExecListener
from pbridge.shared.discovery_models import RegisterDiscovery
from pbridge.shared.exec_models import ExecStatus
from pbridge.shared.jsonline import AsyncJsonLineCodec

from conftest import AsyncRunner, free_port, wait_for


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _bg_register(
    discovery_port: int,
    exec_port: int,
    instance_id: str = "c1",
    instance_name: str = "test",
    alias: Optional[str] = None,
    disconnect_event: Optional[threading.Event] = None,
) -> threading.Event:
    """Register a client in a background thread, return event that fires when done.

    If disconnect_event is provided, the connection closes when it is set.
    Otherwise the connection stays alive for 5 seconds.
    """
    done = threading.Event()

    def _run() -> None:
        async def _do() -> None:
            reader, writer = await asyncio.open_connection("localhost", discovery_port)
            try:
                msg = RegisterDiscovery(
                    pid=1, instance_id=instance_id, instance_name=instance_name,
                    exec_host="localhost", exec_port=exec_port, alias=alias,
                )
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                done.set()
                if disconnect_event is not None:
                    await asyncio.get_event_loop().run_in_executor(
                        None, disconnect_event.wait,
                    )
                else:
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


@pytest.fixture
def listener_runner(exec_port: int) -> Iterator[AsyncRunner]:
    executor = CodeExecutor()
    code_runner = DirectRunner(executor)
    listener = ExecListener("localhost", exec_port, code_runner)
    runner = AsyncRunner()
    runner.start(listener.run)
    yield runner
    listener.stop()
    runner.stop()


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_execute_unknown_client(app_runner) -> None:
    app, runner = app_runner
    wf_id = app.control.start_workflow("test")
    with pytest.raises(KeyError, match="unknown client"):
        runner.run_async(app.control.execute("nonexistent", "print(1)", wf_id))


def test_execute_on_client(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = app.control.start_workflow("test")

    result = app_run.run_async(app.control.execute("c1", 'print("hello")', wf_id))
    assert result.status == ExecStatus.SUCCEEDED
    assert result.stdout == "hello\n"


def test_list_clients_after_register(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port, instance_name="myapp")

    clients = app_run.run_async(app.control.list_clients())
    assert "c1" in clients
    assert clients["c1"].instance_name == "myapp"
    assert clients["c1"].exec_host == "localhost"
    assert clients["c1"].exec_port == exec_port


def test_alias_from_registration(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port, alias="my-alias")

    clients = app_run.run_async(app.control.list_clients())
    assert clients["c1"].alias == "my-alias"


def test_set_alias(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port)

    assert app_run.run_async(app.control.list_clients())["c1"].alias is None
    app_run.run_async(app.control.set_alias("c1", "new-alias"))
    assert app_run.run_async(app.control.list_clients())["c1"].alias == "new-alias"


def test_set_alias_clear(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port, alias="old")

    app_run.run_async(app.control.set_alias("c1", None))
    assert app_run.run_async(app.control.list_clients())["c1"].alias is None


def test_set_alias_unknown_client(app_runner) -> None:
    app, runner = app_runner
    with pytest.raises(KeyError, match="unknown client"):
        runner.run_async(app.control.set_alias("nonexistent", "alias"))


def test_client_disconnect_removes_entry(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    disconnect = threading.Event()
    _bg_register(discovery_port, exec_port, disconnect_event=disconnect)

    assert "c1" in app_run.run_async(app.control.list_clients())
    disconnect.set()
    assert wait_for(lambda: "c1" not in app_run.run_async(app.control.list_clients())), \
        "client entry not removed after disconnect"


def test_evict_stale_on_register(
    app_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner

    stale = ClientEntry(
        pid=0, instance_id="stale-1", instance_name="stale",
        exec_host="localhost", exec_port=0, alias=None,
        last_heartbeat=time.monotonic() - 9999,
    )
    app_run.run_async(app._discovery.register(stale))

    # Register a new client — should evict the stale one
    _bg_register(discovery_port, exec_port)
    clients = app_run.run_async(app.control.list_clients())
    assert "c1" in clients
    assert "stale-1" not in clients
