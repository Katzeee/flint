from __future__ import annotations

import pytest

from python_bridge_mcp.server.control_models import (
    ControlError,
    ErrorResponse,
    GetWorkflowExecutionRequest,
    GetWorkflowExecutionResponse,
    ListInstancesRequest,
    ListInstancesResponse,
    PingRequest,
    PingResponse,
    StartWorkflowRequest,
    StartWorkflowResponse,
    InstanceInfo,
)
from python_bridge_mcp.shared.model_base import VersionedWireModel


# ---------------------------------------------------------------------------
# ListInstances
# ---------------------------------------------------------------------------

def test_list_instances_request_roundtrip() -> None:
    req = ListInstancesRequest(instance_type="maya")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ListInstancesRequest)
    assert parsed.instance_type == "maya"


def test_list_instances_request_no_filter() -> None:
    req = ListInstancesRequest()
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ListInstancesRequest)
    assert parsed.instance_type is None


def test_list_instances_response_roundtrip() -> None:
    resp = ListInstancesResponse(instances=[
        InstanceInfo(instance_id="c1", instance_name="Maya 2024", instance_type="maya"),
    ])
    data = resp.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ListInstancesResponse)
    assert len(parsed.instances) == 1
    assert parsed.instances[0].instance_id == "c1"
    assert parsed.instances[0].instance_type == "maya"


# ---------------------------------------------------------------------------
# StartWorkflow
# ---------------------------------------------------------------------------

def test_start_workflow_request_roundtrip() -> None:
    req = StartWorkflowRequest(name="my-wf", description="test run")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, StartWorkflowRequest)
    assert parsed.name == "my-wf"
    assert parsed.description == "test run"


def test_start_workflow_request_default_description() -> None:
    req = StartWorkflowRequest(name="wf")
    assert req.description == ""


def test_start_workflow_response_roundtrip() -> None:
    resp = StartWorkflowResponse(workflow_id="wf_20240101_abcd1234")
    data = resp.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, StartWorkflowResponse)
    assert parsed.workflow_id == "wf_20240101_abcd1234"


# ---------------------------------------------------------------------------
# GetWorkflowExecution
# ---------------------------------------------------------------------------

def test_get_workflow_execution_request_roundtrip() -> None:
    req = GetWorkflowExecutionRequest(workflow_id="wf-123", execution_id="0001", view="full")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, GetWorkflowExecutionRequest)
    assert parsed.workflow_id == "wf-123"
    assert parsed.execution_id == "0001"
    assert parsed.view == "full"


def test_get_workflow_execution_request_default_view() -> None:
    req = GetWorkflowExecutionRequest(workflow_id="wf-123", execution_id="0001")
    assert req.view == "summary"


def test_get_workflow_execution_response_roundtrip() -> None:
    resp = GetWorkflowExecutionResponse(
        execution_id="0001",
        workflow_id="wf-123",
        name="step-1",
        instance_id="c1",
        status="succeeded",
        stdout="hello\n",
        stderr="",
        started_at="2024-01-01T00:00:00+00:00",
        code="print('hello')",
    )
    data = resp.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, GetWorkflowExecutionResponse)
    assert parsed.execution_id == "0001"
    assert parsed.stdout == "hello\n"
    assert parsed.code == "print('hello')"


def test_get_workflow_execution_response_code_none_in_summary() -> None:
    resp = GetWorkflowExecutionResponse(
        execution_id="0001", workflow_id="wf-123", name="step-1",
        instance_id="c1", status="succeeded",
        stdout="hello\n", stderr="", started_at="2024-01-01T00:00:00+00:00",
        code=None,
    )
    data = resp.to_dict(exclude_none=True)
    assert "code" not in data


def test_ping_roundtrip() -> None:
    payload = PingRequest().to_dict()
    parsed = VersionedWireModel.parse_versioned(payload)
    assert isinstance(parsed, PingRequest)


def test_pong_contains_backend_identity() -> None:
    payload = PingResponse(ok=True, service="python-bridge-backend", ready=True).to_dict()
    parsed = VersionedWireModel.parse_versioned(payload)
    assert isinstance(parsed, PingResponse)
    assert parsed.service == "python-bridge-backend"


def test_error_response_roundtrip_casts_control_error() -> None:
    payload = ErrorResponse(
        error_code=ControlError.WORKFLOW_NOT_FOUND,
        message="missing workflow",
    ).to_dict()
    parsed = VersionedWireModel.parse_versioned(payload)
    assert isinstance(parsed, ErrorResponse)
    assert parsed.error_code == ControlError.WORKFLOW_NOT_FOUND
    assert payload["error_code"] == "workflow_not_found"


