from __future__ import annotations

import json

import pytest

from python_bridge_mcp.shared.instance_control_models import InstanceExecStatus
from python_bridge_mcp.shared.workflow_models import WorkflowRecord
from python_bridge_mcp.shared.workflow_persistence import WorkflowPersistence


def _read_record(workflow_id: str) -> WorkflowRecord:
    path = WorkflowPersistence.resolve(workflow_id)
    with open(path, encoding="utf-8") as f:
        return WorkflowRecord.from_dict(json.load(f))


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_create_workflow() -> None:
    wf_id = WorkflowPersistence.create_workflow("my-workflow")
    assert "my-workflow" in wf_id

    record = _read_record(wf_id)
    assert record.workflow_id == wf_id
    assert record.name == "my-workflow"
    assert record.execs == []


def test_create_workflow_ids_are_unique() -> None:
    wf1 = WorkflowPersistence.create_workflow("wf")
    wf2 = WorkflowPersistence.create_workflow("wf")
    assert wf1 != wf2


def test_resolve() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    path = WorkflowPersistence.resolve(wf_id)
    assert path.endswith(f"{wf_id}.json")


def test_exists() -> None:
    assert not WorkflowPersistence.exists("nonexistent")

    wf_id = WorkflowPersistence.create_workflow("test")
    assert WorkflowPersistence.exists(wf_id)


def test_append_running_execution() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step-1", "c1", "print(1)")

    assert exec_id == "0001"
    record = _read_record(wf_id)
    assert len(record.execs) == 1
    entry = record.execs[0]
    assert entry.execution_id == "0001"
    assert entry.name == "step-1"
    assert entry.workflow_id == wf_id
    assert entry.instance_id == "c1"
    assert entry.code == "print(1)"
    assert entry.status == InstanceExecStatus.RUNNING
    assert entry.stdout == ""
    assert entry.stderr == ""


def test_execution_ids_are_sequential() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    id1 = WorkflowPersistence.append_running_execution(wf_id, "first", "c1", "x = 1")
    id2 = WorkflowPersistence.append_running_execution(wf_id, "second", "c1", "x = 2")
    id3 = WorkflowPersistence.append_running_execution(wf_id, "third", "c1", "x = 3")

    assert id1 == "0001"
    assert id2 == "0002"
    assert id3 == "0003"


def test_append_adds_instance_id_once() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    WorkflowPersistence.append_running_execution(wf_id, "a", "c1", "x = 1")
    WorkflowPersistence.append_running_execution(wf_id, "b", "c1", "x = 2")

    record = _read_record(wf_id)
    assert record.instance_ids.count("c1") == 1
    assert len(record.execs) == 2


def test_update_execution_output() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "print(1)")

    WorkflowPersistence.update_execution_output(wf_id, exec_id, "hello\n", "warn\n")

    record = _read_record(wf_id)
    entry = record.execs[0]
    assert entry.stdout == "hello\n"
    assert entry.stderr == "warn\n"
    assert entry.status == InstanceExecStatus.RUNNING


def test_append_execution_output_adds_deltas() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "print(1)")

    WorkflowPersistence.append_execution_output(wf_id, exec_id, "hel", "wa")
    WorkflowPersistence.append_execution_output(wf_id, exec_id, "lo\n", "rn\n")

    record = _read_record(wf_id)
    entry = record.execs[0]
    assert entry.stdout == "hello\n"
    assert entry.stderr == "warn\n"
    assert entry.status == InstanceExecStatus.RUNNING


def test_finalize_execution_result_preserves_output() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "print(1)")
    WorkflowPersistence.append_execution_output(wf_id, exec_id, "hello\n", "")

    WorkflowPersistence.finalize_execution_result(
        wf_id,
        exec_id,
        InstanceExecStatus.SUCCEEDED,
        "2023-11-14T22:13:20+00:00",
    )

    record = _read_record(wf_id)
    entry = record.execs[0]
    assert entry.status == InstanceExecStatus.SUCCEEDED
    assert entry.stdout == "hello\n"
    assert entry.finished_at == "2023-11-14T22:13:20+00:00"


def test_update_execution_result() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "print(1)")

    WorkflowPersistence.update_execution_result(
        wf_id, exec_id, InstanceExecStatus.SUCCEEDED,
        "hello\n", "", "2023-11-14T22:13:20+00:00",
    )

    record = _read_record(wf_id)
    entry = record.execs[0]
    assert entry.status == InstanceExecStatus.SUCCEEDED
    assert entry.stdout == "hello\n"
    assert entry.finished_at == "2023-11-14T22:13:20+00:00"
    assert entry.traceback is None
    assert entry.error is None


def test_update_execution_result_with_error() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    exec_id = WorkflowPersistence.append_running_execution(wf_id, "step", "c1", "raise ValueError()")

    WorkflowPersistence.update_execution_result(
        wf_id, exec_id, InstanceExecStatus.FAILED,
        "", "", "2023-11-14T22:13:20+00:00",
        traceback="Traceback ...\nValueError\n",
        error="ValueError",
    )

    record = _read_record(wf_id)
    entry = record.execs[0]
    assert entry.status == InstanceExecStatus.FAILED
    assert entry.traceback is not None
    assert "ValueError" in entry.traceback
    assert entry.error == "ValueError"


def test_resolve_nonexistent_raises() -> None:
    from python_bridge_mcp.shared.workflow_persistence import WorkflowRecordUnavailableError
    with pytest.raises(WorkflowRecordUnavailableError):
        WorkflowPersistence.resolve("nonexistent-workflow-id-that-does-not-exist")


def test_multiple_execs_in_one_workflow() -> None:
    wf_id = WorkflowPersistence.create_workflow("test")
    id1 = WorkflowPersistence.append_running_execution(wf_id, "first", "c1", "x = 1")
    id2 = WorkflowPersistence.append_running_execution(wf_id, "second", "c2", "x = 2")

    WorkflowPersistence.update_execution_result(
        wf_id, id1, InstanceExecStatus.SUCCEEDED, "1\n", "", "2023-11-14T22:13:20+00:00",
    )
    WorkflowPersistence.update_execution_result(
        wf_id, id2, InstanceExecStatus.FAILED, "", "err", "2023-11-14T22:13:21+00:00",
        error="RuntimeError",
    )

    record = _read_record(wf_id)
    assert len(record.execs) == 2
    assert record.execs[0].status == InstanceExecStatus.SUCCEEDED
    assert record.execs[1].status == InstanceExecStatus.FAILED
    assert set(record.instance_ids) == {"c1", "c2"}
