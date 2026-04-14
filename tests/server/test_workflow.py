import asyncio
import threading
from typing import Iterator

import pytest

from python_bridge_mcp.server.app import App
from python_bridge_mcp.shared.workflow_persistence import WorkflowRecordUnavailableError
from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.exec_listener import ExecListener
from python_bridge_mcp.shared.discovery_models import RegisterDiscovery
from python_bridge_mcp.shared.exec_models import ExecStatus
from python_bridge_mcp.shared.jsonline import AsyncJsonLineCodec

from conftest import AsyncRunner, free_port


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _bg_register(
    discovery_port: int,
    exec_port: int,
    instance_id: str = "c1",
) -> threading.Event:
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

def test_start_workflow_returns_id_with_name(app_runner) -> None:
    app, runner = app_runner
    wf_id = app.control.start_workflow("my-workflow")
    assert isinstance(wf_id, str)
    assert "my-workflow" in wf_id


def test_start_workflow_ids_are_unique(app_runner) -> None:
    app, runner = app_runner
    wf1 = app.control.start_workflow("wf")
    wf2 = app.control.start_workflow("wf")
    assert wf1 != wf2


def test_execute_with_workflow_id(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = app.control.start_workflow("test")

    result = app_run.run_async(app.control.execute("c1", 'print("hello")', wf_id))
    assert result.status == ExecStatus.SUCCEEDED
    assert result.stdout == "hello\n"


def test_execute_rejects_missing_workflow(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    app, app_run = app_runner
    _bg_register(discovery_port, exec_port)

    with pytest.raises(WorkflowRecordUnavailableError, match="workflow not found"):
        app_run.run_async(app.control.execute("c1", 'print("ok")', "nonexistent"))
