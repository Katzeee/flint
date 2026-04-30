import asyncio
import json
import threading
import time
from typing import Iterator

import pytest

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import Registry
from python_bridge_mcp.shared.instance_control_models import InstanceExecStatus
from python_bridge_mcp.shared.workflow_models import WorkflowRecord
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence

from conftest import AsyncRunner, free_port, wait_for


@pytest.fixture
def port() -> int:
    return free_port()


class _ClientRunner:
    def __init__(self, client: DiscoveryClient) -> None:
        self.client = client
        self._thread = threading.Thread(target=client.run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self.client.stop()
        self._thread.join(timeout=5)


@pytest.fixture
def connected_system(port: int) -> Iterator[tuple]:
    registry = Registry(host="localhost", port=port)
    control = ControlServer(registry)
    server = AsyncRunner()
    server.start(registry.run)

    client = DiscoveryClient(
        instance_id="c1",
        instance_name="test",
        runner=DirectRunner(CodeExecutor()),
        host="localhost",
        port=port,
        heartbeat_interval=0.1,
        pid=1001,
    )
    client_runner = _ClientRunner(client)
    client_runner.start()
    assert client.wait_until_registered(timeout=3)

    try:
        yield server, registry, control, client
    finally:
        client_runner.stop()
        registry.stop()
        server.stop()


def _read_exec_status(wf_id: str) -> InstanceExecStatus:
    path = WorkflowPersistence.resolve(wf_id)
    with open(path, encoding="utf-8") as f:
        record = WorkflowRecord.from_dict(json.load(f))
    return record.execs[0].status


def _read_first_exec(wf_id: str):
    path = WorkflowPersistence.resolve(wf_id)
    with open(path, encoding="utf-8") as f:
        record = WorkflowRecord.from_dict(json.load(f))
    return record.execs[0]


def test_exec_hello_world(connected_system) -> None:
    server, _, control, _ = connected_system
    wf_id = WorkflowPersistence.create_workflow("exec-test")

    result = server.run_async(control.execute("c1", 'print("hello")', wf_id))

    assert result.status == InstanceExecStatus.SUCCEEDED
    assert result.traceback is None
    entry = _read_first_exec(wf_id)
    assert entry.stdout == "hello\n"
    assert entry.stderr == ""


def test_exec_exception(connected_system) -> None:
    server, _, control, _ = connected_system
    wf_id = WorkflowPersistence.create_workflow("exec-error-test")

    result = server.run_async(control.execute("c1", "raise ValueError('boom')", wf_id))

    assert result.status == InstanceExecStatus.FAILED
    assert result.traceback is not None
    assert "ValueError" in result.traceback
    assert "boom" in result.traceback


def test_exec_namespace_persists(connected_system) -> None:
    server, _, control, _ = connected_system
    wf_id = WorkflowPersistence.create_workflow("namespace-test")

    first = server.run_async(control.execute("c1", "x = 42", wf_id))
    second = server.run_async(control.execute("c1", "print(x)", wf_id))

    assert first.status == InstanceExecStatus.SUCCEEDED
    assert second.status == InstanceExecStatus.SUCCEEDED
    assert _read_first_exec(wf_id).status == InstanceExecStatus.SUCCEEDED
    record = WorkflowPersistence.load(wf_id)
    assert record.execs[1].stdout == "42\n"


def test_exec_concurrent_rejects_busy(connected_system) -> None:
    server, _, control, _ = connected_system
    wf_id = WorkflowPersistence.create_workflow("busy-test")

    async def _run():
        first = asyncio.create_task(
            control.execute("c1", 'import time; time.sleep(0.3); print("a")', wf_id)
        )
        await asyncio.sleep(0.05)
        second = asyncio.create_task(control.execute("c1", 'print("b")', wf_id))
        return await asyncio.gather(first, second)

    first, second = server.run_async(_run())

    assert first.status == InstanceExecStatus.SUCCEEDED
    assert _read_first_exec(wf_id).stdout.strip() == "a"
    assert second.status == InstanceExecStatus.FAILED
    assert second.error == "busy"


def test_early_return_result_persisted_to_disk(connected_system) -> None:
    server, _, control, _ = connected_system
    wf_id = WorkflowPersistence.create_workflow("early-return-test")

    result = server.run_async(
        control.execute(
            "c1",
            'import time; time.sleep(0.3); print("done")',
            wf_id,
            early_return_window=0.05,
        )
    )

    assert result.status == InstanceExecStatus.RUNNING
    assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.SUCCEEDED)


def test_set_alias_roundtrip(connected_system) -> None:
    server, registry, control, client = connected_system

    result = server.run_async(control.set_alias("c1", "lighting"))

    assert result.success is True
    assert result.alias == "lighting"
    assert client._current_alias() == "lighting"
    assert registry.list_clients()["c1"].alias == "lighting"


def test_disconnect_marks_running_execution_failed(connected_system) -> None:
    server, registry, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("disconnect-test")

    result = server.run_async(
        control.execute(
            "c1",
            'import time; time.sleep(1.0); print("late")',
            wf_id,
            early_return_window=0.05,
        )
    )
    assert result.status == InstanceExecStatus.RUNNING

    client.stop()

    assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.FAILED)
    assert wait_for(lambda: "c1" not in registry.list_clients())
