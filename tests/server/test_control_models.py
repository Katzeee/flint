from __future__ import annotations

import pytest

from python_bridge_mcp.server.control_models import (
    GetWorkflowExecutionRequest,
    GetWorkflowExecutionResponse,
    GetWorkflowOverviewRequest,
    GetWorkflowOverviewResponse,
    ListTargetsRequest,
    ListTargetsResponse,
    SetTargetAliasRequest,
    SetTargetAliasResponse,
    StartWorkflowRequest,
    StartWorkflowResponse,
    TargetInfo,
)
from python_bridge_mcp.shared.model_base import VersionedWireModel


# ---------------------------------------------------------------------------
# ListTargets
# ---------------------------------------------------------------------------

def test_list_targets_request_roundtrip() -> None:
    req = ListTargetsRequest(instance_type="maya")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ListTargetsRequest)
    assert parsed.instance_type == "maya"


def test_list_targets_request_no_filter() -> None:
    req = ListTargetsRequest()
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ListTargetsRequest)
    assert parsed.instance_type is None


def test_list_targets_response_roundtrip() -> None:
    resp = ListTargetsResponse(targets=[
        TargetInfo(instance_id="c1", instance_name="Maya 2024",
                   exec_host="localhost", exec_port=8001, instance_type="maya"),
    ])
    data = resp.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, ListTargetsResponse)
    assert len(parsed.targets) == 1
    assert parsed.targets[0].instance_id == "c1"
    assert parsed.targets[0].instance_type == "maya"


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
# GetWorkflowOverview
# ---------------------------------------------------------------------------

def test_get_workflow_overview_request_roundtrip() -> None:
    req = GetWorkflowOverviewRequest(workflow_id="wf-123")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, GetWorkflowOverviewRequest)
    assert parsed.workflow_id == "wf-123"


def test_get_workflow_overview_response_roundtrip() -> None:
    resp = GetWorkflowOverviewResponse(
        workflow_id="wf-123",
        name="my-wf",
        execution_count=3,
        created_at="2024-01-01T00:00:00+00:00",
        description="a test workflow",
        instance_ids=["c1", "c2"],
    )
    data = resp.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, GetWorkflowOverviewResponse)
    assert parsed.workflow_id == "wf-123"
    assert parsed.name == "my-wf"
    assert parsed.execution_count == 3
    assert parsed.created_at == "2024-01-01T00:00:00+00:00"
    assert parsed.description == "a test workflow"
    assert parsed.instance_ids == ["c1", "c2"]


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


# ---------------------------------------------------------------------------
# SetTargetAlias
# ---------------------------------------------------------------------------

def test_set_target_alias_request_roundtrip() -> None:
    req = SetTargetAliasRequest(instance_id="c1", alias="my-maya")
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, SetTargetAliasRequest)
    assert parsed.instance_id == "c1"
    assert parsed.alias == "my-maya"


def test_set_target_alias_request_clear_alias() -> None:
    req = SetTargetAliasRequest(instance_id="c1", alias=None)
    data = req.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, SetTargetAliasRequest)
    assert parsed.alias is None


def test_set_target_alias_response_roundtrip() -> None:
    resp = SetTargetAliasResponse(success=True, instance_id="c1", alias="my-maya")
    data = resp.to_dict()
    parsed = VersionedWireModel.parse_versioned(data)
    assert isinstance(parsed, SetTargetAliasResponse)
    assert parsed.success is True
    assert parsed.instance_id == "c1"
    assert parsed.alias == "my-maya"


def test_set_target_alias_response_cleared_alias() -> None:
    resp = SetTargetAliasResponse(success=True, instance_id="c1", alias=None)
    data = resp.to_dict(exclude_none=True)
    parsed = VersionedWireModel.parse_versioned(data)
    assert parsed.alias is None
