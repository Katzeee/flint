import asyncio
import threading
from typing import Iterator

import pytest

from python_bridge_mcp.server.registry import ClientEntry, Registry
from python_bridge_mcp.server.control_server import ControlServer
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence, WorkflowRecordUnavailableError
from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import DirectRunner
from python_bridge_mcp.client.exec_listener import ExecListener
from python_bridge_mcp.shared.discovery_models import RegisterDiscovery
from python_bridge_mcp.shared.exec_models import ExecError, ExecStatus
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
    registry = Registry(host="localhost", port=discovery_port)
    control = ControlServer(registry)
    runner = AsyncRunner()
    runner.start(registry.run)
    yield registry, control, runner
    registry.stop()
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
    registry, control, runner = app_runner
    wf_id = control.start_workflow("my-workflow")
    assert isinstance(wf_id, str)
    assert "my-workflow" in wf_id


def test_start_workflow_ids_are_unique(app_runner) -> None:
    registry, control, runner = app_runner
    wf1 = control.start_workflow("wf")
    wf2 = control.start_workflow("wf")
    assert wf1 != wf2


def test_execute_with_workflow_id(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = control.start_workflow("test")

    result = app_run.run_async(control.execute("c1", 'print("hello")', wf_id))
    assert result.status == ExecStatus.SUCCEEDED
    assert result.stdout == "hello\n"


def test_execute_rejects_missing_workflow(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port)

    with pytest.raises(WorkflowRecordUnavailableError, match="workflow not found"):
        app_run.run_async(control.execute("c1", 'print("ok")', "nonexistent"))


# ---------------------------------------------------------------------------
# 1.1 — Connection failure records FAILED execution
# ---------------------------------------------------------------------------

def test_execute_connection_failed_records_failed_execution(app_runner) -> None:
    """When open_connection fails, execution is written as FAILED with CONNECTION_FAILED."""
    registry, control, app_run = app_runner
    # Register a target on a port that is not listening (port 1 is privileged/closed)
    registry.register(ClientEntry(
        pid=1, instance_id="dead", instance_name="dead",
        exec_host="127.0.0.1", exec_port=1, alias=None,
    ))
    wf_id = control.start_workflow("conn-fail-test")

    result = app_run.run_async(
        control.execute("dead", "print(1)", wf_id, connect_timeout=2.0)
    )

    assert result.status == ExecStatus.FAILED
    assert result.error == ExecError.CONNECTION_FAILED

    # Verify the failure is persisted to disk
    record = WorkflowPersistence.load(wf_id)
    assert len(record.execs) == 1
    entry = record.execs[0]
    assert entry.status == ExecStatus.FAILED
    assert entry.error == ExecError.CONNECTION_FAILED.value


# ---------------------------------------------------------------------------
# 1.4 — Registry: reconnect does not evict newer entry
# ---------------------------------------------------------------------------

def test_registry_unregister_pid_guard_unit() -> None:
    """unregister() with a stale PID must not evict a newer entry (unit test)."""
    registry = Registry()
    old_entry = ClientEntry(
        pid=10, instance_id="shared", instance_name="t",
        exec_host="localhost", exec_port=1, alias=None,
    )
    registry.register(old_entry)

    # Simulate re-registration with a new PID
    new_entry = ClientEntry(
        pid=20, instance_id="shared", instance_name="t",
        exec_host="localhost", exec_port=1, alias=None,
    )
    registry.register(new_entry)

    # Old TCP connection closes — passes stale PID=10 → must NOT evict PID=20
    registry.unregister("shared", pid=10)
    entry = registry.get_client("shared")
    assert entry is not None, "Stale PID unregister wrongly evicted new entry"
    assert entry.pid == 20

    # New TCP connection closes — passes correct PID=20 → SHOULD evict
    registry.unregister("shared", pid=20)
    assert registry.get_client("shared") is None


def test_registry_reconnect_does_not_evict_new_entry(discovery_port) -> None:
    """Integration: closing an old TCP connection must not evict a newer registration."""
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
                msg = RegisterDiscovery(
                    pid=pid, instance_id="shared", instance_name="test",
                    exec_host="localhost", exec_port=9999,
                )
                await AsyncJsonLineCodec.send(writer, msg.to_dict())
                await AsyncJsonLineCodec.recv(reader)
                done_event.set()
                await asyncio.sleep(hold_seconds)
            finally:
                writer.close()
        asyncio.run(_do())

    # Old connection (PID=10) holds for 1 second then closes
    t_old = threading.Thread(target=_connect, args=(10, old_done, 1.0), daemon=True)
    t_old.start()
    assert old_done.wait(timeout=3), "old registration failed"

    # New connection (PID=20) holds for 5 seconds (stays open during the check)
    t_new = threading.Thread(target=_connect, args=(20, new_done, 5.0), daemon=True)
    t_new.start()
    assert new_done.wait(timeout=3), "new registration failed"

    # Wait for old connection to close (hold=1.0 s)
    t_old.join(timeout=3)
    time.sleep(0.1)  # let registry process the close

    # PID=20 entry must still be present — old close must not have evicted it
    entry = registry.get_client("shared")
    assert entry is not None, "Entry was wrongly evicted when old connection closed"
    assert entry.pid == 20

    registry.stop()
    runner.stop()


# ---------------------------------------------------------------------------
# 2.2 — request_id tracking
# ---------------------------------------------------------------------------

def test_request_id_present_in_result(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    """execute() generates a request_id that appears in the returned ExecResult."""
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = control.start_workflow("reqid-test")

    result = app_run.run_async(control.execute("c1", 'print("hi")', wf_id))
    assert result.request_id is not None
    assert len(result.request_id) > 0


def test_request_id_persisted_in_exec_entry(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    """The request_id used for the ExecRequest is stored in the ExecEntry on disk."""
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = control.start_workflow("reqid-persist-test")

    result = app_run.run_async(control.execute("c1", 'print("hi")', wf_id))
    record = WorkflowPersistence.load(wf_id)
    assert len(record.execs) == 1
    entry = record.execs[0]
    assert entry.request_id is not None
    assert len(entry.request_id) > 0


# ---------------------------------------------------------------------------
# 2.3 — per-target summaries in get_workflow_overview
# ---------------------------------------------------------------------------

def test_get_workflow_overview_target_summaries(
    app_runner, listener_runner, discovery_port: int, exec_port: int,
) -> None:
    """get_workflow_overview includes per-target exec_count and latest_status."""
    registry, control, app_run = app_runner
    _bg_register(discovery_port, exec_port)
    wf_id = control.start_workflow("overview-test")

    app_run.run_async(control.execute("c1", 'print("a")', wf_id))
    app_run.run_async(control.execute("c1", 'print("b")', wf_id))

    overview = control.get_workflow_overview(wf_id)
    assert len(overview.target_summaries) == 1
    summary = overview.target_summaries[0]
    assert summary.instance_id == "c1"
    assert summary.exec_count == 2
    assert summary.active_count == 0
    assert summary.latest_status == "succeeded"


def test_get_workflow_overview_target_summaries_in_dict(app_runner) -> None:
    """target_summaries appears in the serialized overview dict."""
    registry, control, _ = app_runner
    wf_id = control.start_workflow("overview-dict-test")
    overview = control.get_workflow_overview(wf_id)
    data = overview.to_dict(exclude_none=True)
    assert "target_summaries" in data
    assert data["target_summaries"] == []
