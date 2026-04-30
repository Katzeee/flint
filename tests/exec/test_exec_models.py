from __future__ import annotations

import pytest

from python_bridge_mcp.shared.instance_control_models import (
    InstanceExecError,
    InstanceExecOutputUpdate,
    InstanceExecRequest,
    InstanceExecResult,
    InstanceExecStatus,
    InstanceAck,
    InstanceControlError,
    InstanceControlWireModel,
    InstanceHeartbeat,
    InstanceRegister,
)
from python_bridge_mcp.shared.model_base import VersionedWireModel, WireModelError


def test_exec_request_roundtrip():
    req = InstanceExecRequest(execution_id="r1", code="print(1)", workflow_id="wf-1")
    data = req.to_dict()
    assert data["type"] == "InstanceExecRequest"
    assert data["version"] == 3
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecRequest)
    assert parsed == req


def test_instance_control_messages_share_protocol_version():
    assert InstanceControlWireModel.PROTOCOL_VERSION == 3
    assert InstanceRegister(pid=1, instance_id="i1", instance_name="py").to_dict()["version"] == 3
    assert InstanceHeartbeat(instance_id="i1").to_dict()["version"] == 3
    assert InstanceExecRequest(execution_id="r1", code="x", workflow_id="wf").to_dict()["version"] == 3


def test_instance_ack_error_code_roundtrip():
    ack = InstanceAck(
        success=False,
        error_code=InstanceControlError.NOT_REGISTERED,
        message="instance has not registered",
    )

    data = ack.to_dict()
    assert data["error_code"] == "not_registered"
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceAck)
    assert parsed.error_code == InstanceControlError.NOT_REGISTERED
    assert parsed.message == "instance has not registered"


def test_exec_result_roundtrip():
    res = InstanceExecResult(execution_id="r1", status=InstanceExecStatus.SUCCEEDED)
    data = res.to_dict()
    assert data["type"] == "InstanceExecResult"
    assert data["version"] == 3
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecResult)
    assert parsed == res


def test_exec_result_with_traceback():
    res = InstanceExecResult(
        execution_id="r1",
        status=InstanceExecStatus.FAILED,
        traceback="Traceback ...\nNameError: name 'x' is not defined\n",
    )
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecResult)
    assert parsed.traceback == res.traceback


def test_exec_request_version_mismatch():
    data = {
        "type": "InstanceExecRequest",
        "version": 99,
        "execution_id": "r1",
        "code": "x",
    }
    with pytest.raises(WireModelError, match="Version mismatch"):
        VersionedWireModel.parse_versioned(data)


def test_exec_status_succeeded_value():
    assert InstanceExecStatus.SUCCEEDED.value == "succeeded"


def test_exec_request_execution_name_roundtrip():
    req = InstanceExecRequest(execution_id="r1", code="print(1)", workflow_id="wf-1", execution_name="step-1")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert parsed.execution_name == "step-1"


def test_exec_request_execution_name_default_none():
    req = InstanceExecRequest(execution_id="r1", code="print(1)", workflow_id="wf-1")
    assert req.execution_name is None
    data = req.to_dict(exclude_none=True)
    assert "execution_name" not in data


def test_exec_status_pending_roundtrip():
    res = InstanceExecResult(execution_id="r1", status=InstanceExecStatus.PENDING)
    data = res.to_dict()
    assert data["status"] == "pending"
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecResult)
    assert parsed.status == InstanceExecStatus.PENDING


def test_exec_result_has_no_output_fields():
    res = InstanceExecResult(execution_id="r1", status=InstanceExecStatus.RUNNING)
    data = res.to_dict()
    assert "stdout" not in data
    assert "stderr" not in data


def test_exec_result_running_roundtrip():
    """RUNNING InstanceExecResult serializes and deserializes correctly."""
    res = InstanceExecResult(execution_id="r1", status=InstanceExecStatus.RUNNING)
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecResult)
    assert parsed.status == InstanceExecStatus.RUNNING


def test_exec_output_update_roundtrip():
    update = InstanceExecOutputUpdate(
        execution_id="r1",
        workflow_id="wf",
        sequence=1,
        stdout_delta="hello\n",
        request_id="abc123",
    )
    data = update.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecOutputUpdate)
    assert parsed.stdout_delta == "hello\n"
    assert parsed.sequence == 1


# ---------------------------------------------------------------------------
# 2.1 — Structured error codes
# ---------------------------------------------------------------------------

def test_exec_error_codes_exist():
    assert InstanceExecError.BUSY == "busy"
    assert InstanceExecError.INSTANCE_OFFLINE == "instance_offline"
    assert InstanceExecError.CONNECTION_FAILED == "connection_failed"
    assert InstanceExecError.PROTOCOL_ERROR == "protocol_error"
    assert InstanceExecError.EXECUTION_TIMEOUT == "execution_timeout"


def test_exec_result_error_code_roundtrip():
    """InstanceExecError enum values survive a dict roundtrip."""
    res = InstanceExecResult(
        execution_id="r1",
        status=InstanceExecStatus.FAILED,
        error=InstanceExecError.CONNECTION_FAILED,
    )
    data = res.to_dict()
    assert data["error"] == "connection_failed"
    parsed = VersionedWireModel.parse_versioned(data)
    assert parsed.error == "connection_failed"


# ---------------------------------------------------------------------------
# 2.2 — request_id field
# ---------------------------------------------------------------------------

def test_exec_request_default_request_id():
    req = InstanceExecRequest(execution_id="r1", code="x", workflow_id="wf")
    assert req.request_id is None


def test_exec_request_request_id_roundtrip():
    req = InstanceExecRequest(execution_id="r1", code="x", workflow_id="wf", request_id="abc123")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecRequest)
    assert parsed.request_id == "abc123"


def test_exec_result_request_id_roundtrip():
    res = InstanceExecResult(
        execution_id="r1",
        status=InstanceExecStatus.SUCCEEDED,
        request_id="abc123",
    )
    data = res.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, InstanceExecResult)
    assert parsed.request_id == "abc123"


def test_exec_result_request_id_excluded_when_none():
    res = InstanceExecResult(execution_id="r1", status=InstanceExecStatus.SUCCEEDED)
    assert res.request_id is None
    data = res.to_dict(exclude_none=True)
    assert "request_id" not in data
