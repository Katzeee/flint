from __future__ import annotations

import json

import pytest

from pbridge.shared.exec_models import ExecStatus
from pbridge.shared.workflow_models import WorkflowRecord
from pbridge.shared.workflow_persistence import WorkflowPersistence


def _read_record(path: str) -> WorkflowRecord:
    with open(path, encoding="utf-8") as f:
        return WorkflowRecord.from_dict(json.load(f))


# ---------------------------------------------------------------------------
# Tests
# ---------------------------------------------------------------------------

def test_create_workflow(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "my-workflow")

    record = _read_record(wf_path)
    assert record.workflow_id == "wf-1"
    assert record.name == "my-workflow"
    assert record.execs == []


def test_exists(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    assert not WorkflowPersistence.exists(wf_path)

    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    assert WorkflowPersistence.exists(wf_path)


def test_append_running_execution(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    WorkflowPersistence.append_running_execution(wf_path, "r1", "wf-1", "c1", "print(1)")

    record = _read_record(wf_path)
    assert len(record.execs) == 1
    entry = record.execs[0]
    assert entry.request_id == "r1"
    assert entry.workflow_id == "wf-1"
    assert entry.instance_id == "c1"
    assert entry.code == "print(1)"
    assert entry.status == ExecStatus.RUNNING
    assert entry.stdout == ""
    assert entry.stderr == ""


def test_append_adds_instance_id_once(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    WorkflowPersistence.append_running_execution(wf_path, "r1", "wf-1", "c1", "x = 1")
    WorkflowPersistence.append_running_execution(wf_path, "r2", "wf-1", "c1", "x = 2")

    record = _read_record(wf_path)
    assert record.instance_ids.count("c1") == 1
    assert len(record.execs) == 2


def test_update_execution_output(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    WorkflowPersistence.append_running_execution(wf_path, "r1", "wf-1", "c1", "print(1)")

    WorkflowPersistence.update_execution_output(wf_path, "r1", "hello\n", "warn\n")

    record = _read_record(wf_path)
    entry = record.execs[0]
    assert entry.stdout == "hello\n"
    assert entry.stderr == "warn\n"
    assert entry.status == ExecStatus.RUNNING


def test_update_execution_result(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    WorkflowPersistence.append_running_execution(wf_path, "r1", "wf-1", "c1", "print(1)")

    WorkflowPersistence.update_execution_result(
        wf_path, "r1", ExecStatus.SUCCEED,
        "hello\n", "", 1700000000.0,
    )

    record = _read_record(wf_path)
    entry = record.execs[0]
    assert entry.status == ExecStatus.SUCCEED
    assert entry.stdout == "hello\n"
    assert entry.finished_at == 1700000000.0
    assert entry.traceback is None
    assert entry.error is None


def test_update_execution_result_with_error(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    WorkflowPersistence.append_running_execution(wf_path, "r1", "wf-1", "c1", "raise ValueError()")

    WorkflowPersistence.update_execution_result(
        wf_path, "r1", ExecStatus.FAILED,
        "", "", 1700000000.0,
        traceback="Traceback ...\nValueError\n",
        error="ValueError",
    )

    record = _read_record(wf_path)
    entry = record.execs[0]
    assert entry.status == ExecStatus.FAILED
    assert entry.traceback is not None
    assert "ValueError" in entry.traceback
    assert entry.error == "ValueError"


def test_multiple_execs_in_one_workflow(tmp_path) -> None:
    wf_path = str(tmp_path / "wf.json")
    WorkflowPersistence.create_workflow(wf_path, "wf-1", "test")
    WorkflowPersistence.append_running_execution(wf_path, "r1", "wf-1", "c1", "x = 1")
    WorkflowPersistence.append_running_execution(wf_path, "r2", "wf-1", "c2", "x = 2")

    WorkflowPersistence.update_execution_result(
        wf_path, "r1", ExecStatus.SUCCEED, "1\n", "", 1700000000.0,
    )
    WorkflowPersistence.update_execution_result(
        wf_path, "r2", ExecStatus.FAILED, "", "err", 1700000001.0,
        error="RuntimeError",
    )

    record = _read_record(wf_path)
    assert len(record.execs) == 2
    assert record.execs[0].status == ExecStatus.SUCCEED
    assert record.execs[1].status == ExecStatus.FAILED
    assert set(record.instance_ids) == {"c1", "c2"}
