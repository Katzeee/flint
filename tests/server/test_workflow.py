"""Tests for workflow-specific behavior: missing workflows, connection failures, request IDs."""
import asyncio
import threading
from typing import Iterator

import pytest

from python_bridge_mcp.server.registry import ClientEntry, Registry
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence, WorkflowRecordUnavailableError
from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import CodeRunner
from python_bridge_mcp.client.execution_strategy import DirectExecutionStrategy
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.shared.instance_control_models import InstanceExecError, InstanceExecStatus, InstanceRegister
from python_bridge_mcp.shared.jsonline import AsyncJsonLineCodec

from conftest import AsyncRunner, free_port


class DccRunner:
    def __init__(self, client: DiscoveryClient) -> None:
        self.client = client
        self._thread = threading.Thread(target=client.run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def stop(self) -> None:
        self.client.stop()
        self._thread.join(timeout=5)


def _bg_register(discovery_port: int, name_hint: str = "c1") -> DccRunner:
    client = DiscoveryClient(
        name_hint=name_hint,
        instance_name="test",
        runner=CodeRunner(CodeExecutor(), DirectExecutionStrategy()),
        host="localhost",
        port=discovery_port,
        heartbeat_interval=0.1,
        pid=1,
    )
    runner = DccRunner(client)
    runner.start()
    assert client.wait_until_registered(timeout=3), "registration failed"
    return runner


@pytest.fixture
def discovery_port() -> int:
    return free_port()


@pytest.fixture
def app_runner(discovery_port: int) -> Iterator[tuple]:
    registry = Registry(host="localhost", port=discovery_port)
    control = ControlServer(registry)
    runner = AsyncRunner()
    runner.start(registry.run)
    yield registry, control, runner
    runner.stop()


def test_execute_rejects_missing_workflow(app_runner, discovery_port: int) -> None:
    registry, control, app_run = app_runner
    dcc = _bg_register(discovery_port)
    try:
        with pytest.raises(WorkflowRecordUnavailableError, match="workflow not found"):
            app_run.run_async(control.execute(dcc.client.instance_id, 'print("ok")', "nonexistent"))
    finally:
        dcc.stop()


def test_execute_connection_failed_records_failed_execution(app_runner) -> None:
    registry, control, app_run = app_runner
    registry.register(ClientEntry(pid=1, instance_id="dead", instance_name="dead"))
    wf_id = control.start_workflow("conn-fail-test")

    result = app_run.run_async(control.execute("dead", "print(1)", wf_id, connect_timeout=2.0))

    assert result.status == InstanceExecStatus.FAILED
    assert result.error == InstanceExecError.CONNECTION_FAILED
    record = WorkflowPersistence.load(wf_id)
    assert record.execs[0].status == InstanceExecStatus.FAILED


def test_registry_unregister_pid_guard_unit() -> None:
    registry = Registry()
    registry.register(ClientEntry(pid=10, instance_id="shared", instance_name="t"))
    registry.register(ClientEntry(pid=20, instance_id="shared", instance_name="t"))

    registry.unregister("shared", pid=10)
    entry = registry.get_client("shared")
    assert entry is not None and entry.pid == 20

    registry.unregister("shared", pid=20)
    assert registry.get_client("shared") is None


def test_registry_reconnect_does_not_evict_new_entry(discovery_port) -> None:
    import time

    registry = Registry(host="localhost", port=discovery_port)
    runner = AsyncRunner()
    runner.start(registry.run)

    old_done = threading.Event()
    new_done = threading.Event()

    def _connect(pid: int, done_event: threading.Event, hold_seconds: float) -> None:
        async def _do() -> None:
            reader, writer = await asyncio.open_connection("localhost", discovery_port)
            try:
                msg = InstanceRegister(pid=pid, name_hint="shared", instance_name="test")
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                done_event.set()
                await asyncio.sleep(hold_seconds)
            finally:
                writer.close()
        asyncio.run(_do())

    t_old = threading.Thread(target=_connect, args=(10, old_done, 1.0), daemon=True)
    t_old.start()
    assert old_done.wait(timeout=3)

    t_new = threading.Thread(target=_connect, args=(20, new_done, 5.0), daemon=True)
    t_new.start()
    assert new_done.wait(timeout=3)

    t_old.join(timeout=3)
    time.sleep(0.1)

    clients = registry.list_clients()
    remaining = [e for e in clients.values() if e.pid == 20]
    assert len(remaining) == 1

    runner.stop()


def test_request_id_present_in_result(app_runner, discovery_port: int) -> None:
    registry, control, app_run = app_runner
    dcc = _bg_register(discovery_port)
    try:
        wf_id = control.start_workflow("reqid-test")
        result = app_run.run_async(control.execute(dcc.client.instance_id, 'print("hi")', wf_id))
        assert result.request_id is not None and len(result.request_id) > 0
    finally:
        dcc.stop()


def test_request_id_persisted_in_exec_entry(app_runner, discovery_port: int) -> None:
    registry, control, app_run = app_runner
    dcc = _bg_register(discovery_port)
    try:
        wf_id = control.start_workflow("reqid-persist-test")
        app_run.run_async(control.execute(dcc.client.instance_id, 'print("hi")', wf_id))
        record = WorkflowPersistence.load(wf_id)
        assert record.execs[0].request_id is not None and len(record.execs[0].request_id) > 0
    finally:
        dcc.stop()
