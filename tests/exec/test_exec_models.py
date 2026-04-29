from __future__ import annotations

import pytest

from python_bridge_mcp.shared.exec_models import (
    ExecError,
    ExecOutputUpdate,
    ExecRequest,
    ExecResult,
    ExecStatus,
    ExecWireModel,
)
from python_bridge_mcp.shared.model_base import VersionedWireModel, WireModelError


def test_exec_request_roundtrip():
    req = ExecRequest(execution_id="r1", code="print(1)", workflow_id="wf-1")
    data = req.to_dict()
    assert data["type"] == "ExecRequest"
    assert data["version"] == 3
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecRequest)
    assert parsed == req


def test_exec_result_roundtrip():
    res = ExecResult(execution_id="r1", status=ExecStatus.SUCCEEDED)
    data = res.to_dict()
    assert data["type"] == "ExecResult"
    assert data["version"] == 3
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed == res


def test_exec_result_with_traceback():
    res = ExecResult(
        execution_id="r1",
        status=ExecStatus.FAILED,
        traceback="Traceback ...\nNameError: name 'x' is not defined\n",
    )
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed.traceback == res.traceback


def test_exec_request_version_mismatch():
    data = {
        "type": "ExecRequest",
        "version": 99,
        "execution_id": "r1",
        "code": "x",
    }
    with pytest.raises(WireModelError, match="Version mismatch"):
        VersionedWireModel.parse_versioned(data)


def test_exec_status_succeeded_value():
    assert ExecStatus.SUCCEEDED.value == "succeeded"


def test_exec_request_execution_name_roundtrip():
    req = ExecRequest(execution_id="r1", code="print(1)", workflow_id="wf-1", execution_name="step-1")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert parsed.execution_name == "step-1"


def test_exec_request_execution_name_default_none():
    req = ExecRequest(execution_id="r1", code="print(1)", workflow_id="wf-1")
    assert req.execution_name is None
    data = req.to_dict(exclude_none=True)
    assert "execution_name" not in data


def test_exec_status_pending_roundtrip():
    res = ExecResult(execution_id="r1", status=ExecStatus.PENDING)
    data = res.to_dict()
    assert data["status"] == "pending"
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed.status == ExecStatus.PENDING


def test_exec_result_has_no_output_fields():
    res = ExecResult(execution_id="r1", status=ExecStatus.RUNNING)
    data = res.to_dict()
    assert "stdout" not in data
    assert "stderr" not in data


def test_exec_result_running_roundtrip():
    """RUNNING ExecResult serializes and deserializes correctly."""
    res = ExecResult(execution_id="r1", status=ExecStatus.RUNNING)
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed.status == ExecStatus.RUNNING


def test_exec_output_update_roundtrip():
    update = ExecOutputUpdate(
        execution_id="r1",
        workflow_id="wf",
        sequence=1,
        stdout_delta="hello\n",
        request_id="abc123",
    )
    data = update.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecOutputUpdate)
    assert parsed.stdout_delta == "hello\n"
    assert parsed.sequence == 1


# ---------------------------------------------------------------------------
# 2.1 — Structured error codes
# ---------------------------------------------------------------------------

def test_exec_error_codes_exist():
    assert ExecError.BUSY == "busy"
    assert ExecError.TARGET_OFFLINE == "target_offline"
    assert ExecError.CONNECTION_FAILED == "connection_failed"
    assert ExecError.PROTOCOL_ERROR == "protocol_error"
    assert ExecError.EXECUTION_TIMEOUT == "execution_timeout"


def test_exec_result_error_code_roundtrip():
    """ExecError enum values survive a dict roundtrip."""
    res = ExecResult(
        execution_id="r1",
        status=ExecStatus.FAILED,
        error=ExecError.CONNECTION_FAILED,
    )
    data = res.to_dict()
    assert data["error"] == "connection_failed"
    parsed = VersionedWireModel.parse_versioned(data)
    assert parsed.error == "connection_failed"


# ---------------------------------------------------------------------------
# 2.2 — request_id field
# ---------------------------------------------------------------------------

def test_exec_request_default_request_id():
    req = ExecRequest(execution_id="r1", code="x", workflow_id="wf")
    assert req.request_id is None


def test_exec_request_request_id_roundtrip():
    req = ExecRequest(execution_id="r1", code="x", workflow_id="wf", request_id="abc123")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecRequest)
    assert parsed.request_id == "abc123"


def test_exec_result_request_id_roundtrip():
    res = ExecResult(
        execution_id="r1",
        status=ExecStatus.SUCCEEDED,
        request_id="abc123",
    )
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ExecResult)
    assert parsed.request_id == "abc123"


def test_exec_result_request_id_excluded_when_none():
    res = ExecResult(execution_id="r1", status=ExecStatus.SUCCEEDED)
    assert res.request_id is None
    data = res.to_dict(exclude_none=True)
    assert "request_id" not in data
