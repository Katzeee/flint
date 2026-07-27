import asyncio
import json
import threading
import time
from typing import Iterator

import pytest

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import CodeRunner
from python_bridge_mcp.client.execution_strategy import DirectExecutionStrategy
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.server.registry import Registry
from python_bridge_mcp.shared.instance_control_models import (
    InstanceAck,
    InstanceExecRegister,
    InstanceExecResult,
    InstanceExecStatus,
    InstanceRegister,
)
from python_bridge_mcp.shared.jsonline import AsyncJsonLineCodec
from python_bridge_mcp.shared.model_base import VersionedWireModel
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
        name_hint="c1",
        instance_name="test",
        runner=CodeRunner(CodeExecutor(), DirectExecutionStrategy()),
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
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("exec-test")

    result = server.run_async(control.execute(client.instance_id, 'print("hello")', wf_id))

    assert result.status == InstanceExecStatus.SUCCEEDED
    assert result.traceback is None
    entry = _read_first_exec(wf_id)
    assert entry.stdout == "hello\n"
    assert entry.stderr == ""


def test_exec_exception(connected_system) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("exec-error-test")

    result = server.run_async(control.execute(client.instance_id, "raise ValueError('boom')", wf_id))

    assert result.status == InstanceExecStatus.FAILED
    assert result.traceback is not None
    assert "ValueError" in result.traceback
    assert "boom" in result.traceback


def test_exec_namespace_persists(connected_system) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("namespace-test")

    first = server.run_async(control.execute(client.instance_id, "x = 42", wf_id))
    second = server.run_async(control.execute(client.instance_id, "print(x)", wf_id))

    assert first.status == InstanceExecStatus.SUCCEEDED
    assert second.status == InstanceExecStatus.SUCCEEDED
    assert _read_first_exec(wf_id).status == InstanceExecStatus.SUCCEEDED
    record = WorkflowPersistence.load(wf_id)
    assert record.execs[1].stdout == "42\n"


def test_exec_concurrent_rejects_busy(connected_system) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("busy-test")

    async def _run():
        first = asyncio.create_task(
            control.execute(client.instance_id, 'import time; time.sleep(0.3); print("a")', wf_id)
        )
        await asyncio.sleep(0.05)
        second = asyncio.create_task(control.execute(client.instance_id, 'print("b")', wf_id))
        return await asyncio.gather(first, second)

    first, second = server.run_async(_run())

    assert first.status == InstanceExecStatus.SUCCEEDED
    assert _read_first_exec(wf_id).stdout.strip() == "a"
    assert second.status == InstanceExecStatus.FAILED
    assert second.error == "busy"


def test_exec_retry_after_running_response_is_still_busy(connected_system) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("busy-after-running-test")

    first = server.run_async(
        control.execute(
            client.instance_id,
            'import time; time.sleep(0.4); print("first")',
            wf_id,
            early_return_window=0.05,
        )
    )
    assert first.status == InstanceExecStatus.RUNNING

    # Even though the first MCP-facing call has returned, the DCC execution
    # lock remains held. A retry using a different implementation must not run.
    second = server.run_async(
        control.execute(
            client.instance_id,
            'print("different method")',
            wf_id,
        )
    )

    assert second.status == InstanceExecStatus.FAILED
    assert second.error == "busy"
    assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.SUCCEEDED)
    record = WorkflowPersistence.load(wf_id)
    assert record.execution_count == 1
    assert record.execs[0].stdout.strip() == "first"


def test_early_return_result_persisted_to_disk(connected_system) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("early-return-test")

    result = server.run_async(
        control.execute(
            client.instance_id,
            'import time; time.sleep(0.3); print("done")',
            wf_id,
            early_return_window=0.05,
        )
    )

    assert result.status == InstanceExecStatus.RUNNING
    assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.SUCCEEDED)


def test_background_execution_timeout_fails_log_without_blocking_mcp(
    connected_system,
    monkeypatch,
) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("background-timeout-test")
    monkeypatch.setattr(ControlServer, "BACKGROUND_EXEC_TIMEOUT", 0.15)

    started_at = time.monotonic()
    result = server.run_async(
        control.execute(
            client.instance_id,
            "import time; time.sleep(0.5)",
            wf_id,
            early_return_window=0.03,
        )
    )
    elapsed = time.monotonic() - started_at

    assert result.status == InstanceExecStatus.RUNNING
    assert elapsed < 0.3
    assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.FAILED)
    entry = _read_first_exec(wf_id)
    assert entry.error == "execution_timeout"


def test_disconnect_while_sending_result_marks_execution_failed(
    connected_system,
    monkeypatch,
) -> None:
    server, _, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("result-disconnect-test")
    original_send = client._send_exec

    async def disconnect_before_result(msg):
        if isinstance(msg, InstanceExecResult):
            if client._exec_writer is not None:
                client._exec_writer.close()
            raise ConnectionError("simulated disconnect while sending result")
        await original_send(msg)

    monkeypatch.setattr(client, "_send_exec", disconnect_before_result)
    result = server.run_async(
        control.execute(client.instance_id, "x = 1", wf_id)
    )

    assert result.status == InstanceExecStatus.FAILED
    assert result.error == "connection_failed"
    entry = _read_first_exec(wf_id)
    assert entry.status == InstanceExecStatus.FAILED
    assert entry.error == "connection_failed"


def test_disconnect_marks_running_execution_failed(connected_system) -> None:
    server, registry, control, client = connected_system
    wf_id = WorkflowPersistence.create_workflow("disconnect-test")

    instance_id = client.instance_id
    result = server.run_async(
        control.execute(
            instance_id,
            'import time; time.sleep(1.0); print("late")',
            wf_id,
            early_return_window=0.05,
        )
    )
    assert result.status == InstanceExecStatus.RUNNING

    client.stop()

    assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.FAILED)
    assert wait_for(lambda: instance_id not in registry.list_clients())


def test_stale_dcc_during_execution_marks_log_failed(port: int) -> None:
    """A DCC that keeps sockets open but stops heartbeating must not leave a
    RUNNING execution behind or block the MCP-facing call."""
    registry = Registry(host="localhost", port=port, stale_timeout=0.2)
    control = ControlServer(registry)
    server = AsyncRunner()
    server.start(registry.run)

    ready = threading.Event()
    request_seen = threading.Event()
    state = {}

    def silent_dcc() -> None:
        async def run() -> None:
            ctrl_reader, ctrl_writer = await asyncio.open_connection("localhost", port)
            exec_writer = None
            try:
                await AsyncJsonLineCodec.send(
                    ctrl_writer,
                    InstanceRegister(
                        pid=4242,
                        name_hint="silent",
                        instance_name="silent",
                    ).to_dict(),
                )
                ack = VersionedWireModel.parse_versioned(
                    await AsyncJsonLineCodec.recv(ctrl_reader)
                )
                assert isinstance(ack, InstanceAck) and ack.success
                state["instance_id"] = ack.instance_id

                exec_reader, exec_writer = await asyncio.open_connection(
                    "localhost",
                    port,
                )
                await AsyncJsonLineCodec.send(
                    exec_writer,
                    InstanceExecRegister(
                        instance_id=ack.instance_id,
                        pid=4242,
                    ).to_dict(),
                )
                exec_ack = VersionedWireModel.parse_versioned(
                    await AsyncJsonLineCodec.recv(exec_reader)
                )
                assert isinstance(exec_ack, InstanceAck) and exec_ack.success
                ready.set()

                # Receive the execution but deliberately send neither heartbeat
                # nor result. Wait for stale eviction to close the exec socket.
                await AsyncJsonLineCodec.recv(exec_reader)
                request_seen.set()
                await exec_reader.read()
            finally:
                ctrl_writer.close()
                if exec_writer is not None:
                    exec_writer.close()

        asyncio.run(run())

    dcc_thread = threading.Thread(target=silent_dcc, daemon=True)
    dcc_thread.start()
    try:
        assert ready.wait(timeout=3)
        wf_id = WorkflowPersistence.create_workflow("stale-dcc-test")
        started_at = time.monotonic()
        result = server.run_async(
            control.execute(
                state["instance_id"],
                "import time; time.sleep(10)",
                wf_id,
                early_return_window=0.03,
            )
        )
        assert result.status == InstanceExecStatus.RUNNING
        assert time.monotonic() - started_at < 0.3
        assert request_seen.wait(timeout=1)

        time.sleep(0.25)

        async def trigger_stale_check() -> None:
            registry.list_clients()

        server.run_async(trigger_stale_check())
        assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.FAILED)
        entry = _read_first_exec(wf_id)
        assert entry.error == "connection_failed"
    finally:
        dcc_thread.join(timeout=2)
        server.stop()


def test_heartbeat_keeps_instance_online_during_long_exec(port: int) -> None:
    """A long execution runs on the exec channel while heartbeats flow on the
    control channel. The instance must stay online and its last_heartbeat keep
    advancing — the exec channel must not starve or block the heartbeat.

    Uses a stale window shorter than the execution so a starved heartbeat would
    actually evict the instance mid-exec."""
    STALE = 0.5
    registry = Registry(host="localhost", port=port, stale_timeout=STALE)
    control = ControlServer(registry)
    server = AsyncRunner()
    server.start(registry.run)

    client = DiscoveryClient(
        name_hint="c1",
        instance_name="test",
        runner=CodeRunner(CodeExecutor(), DirectExecutionStrategy()),
        host="localhost",
        port=port,
        heartbeat_interval=0.1,
        pid=1001,
    )
    cr = _ClientRunner(client)
    cr.start()
    assert client.wait_until_registered(timeout=3)
    instance_id = client.instance_id

    try:
        wf_id = WorkflowPersistence.create_workflow("heartbeat-during-exec")
        started = server.run_async(
            control.execute(
                instance_id,
                "import time; time.sleep(1.5)",
                wf_id,
                early_return_window=0.1,
            )
        )
        assert started.status == InstanceExecStatus.RUNNING

        hb_before = registry.get_client(instance_id).last_heartbeat
        # Elapse past the stale window while the exec is still running. Without
        # a fresh heartbeat the periodic eviction would drop the instance here.
        time.sleep(STALE * 1.5)
        entry_mid = registry.get_client(instance_id)
        assert entry_mid is not None, "instance evicted mid-exec (heartbeat starved?)"
        assert entry_mid.last_heartbeat > hb_before, "no heartbeat arrived during exec"

        assert wait_for(lambda: _read_exec_status(wf_id) == InstanceExecStatus.SUCCEEDED)
        assert instance_id in registry.list_clients()
    finally:
        cr.stop()
        server.stop()
