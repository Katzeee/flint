import asyncio
import json
from unittest.mock import AsyncMock
from datetime import datetime, timezone

import pytest

from python_bridge_mcp.server.backend_client import BackendClient
from python_bridge_mcp.server.control_models import (
    GetWorkflowExecutionResponse,
    GetWorkflowOverviewResponse,
    ListTargetsResponse,
    SetTargetAliasResponse,
    TargetInfo,
    TargetSummary,
)
from python_bridge_mcp.server.shim import mcp as shim_mcp
from python_bridge_mcp.shared.instance_control_models import InstanceExecError, InstanceExecResult, InstanceExecStatus
from python_bridge_mcp.shared.workflow_persistence import (
    WorkflowPersistence,
    WorkflowRecordUnavailableError,
)

from conftest import free_port


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

EXPECTED_TOOLS = {
    "list_dcc_targets",
    "exec_python",
    "start_workflow",
    "get_workflow_overview",
    "get_workflow_execution",
    "set_target_alias",
}


def _mock_client(**kwargs) -> BackendClient:
    """Return an AsyncMock configured as a BackendClient with method overrides."""
    client = AsyncMock(spec=BackendClient)
    for attr, val in kwargs.items():
        getattr(client, attr).return_value = val
    return client


def _patch(monkeypatch, client: BackendClient) -> None:
    monkeypatch.setattr("python_bridge_mcp.server.shim._backend_client", client)


# ---------------------------------------------------------------------------
# E1 — basic framework
# ---------------------------------------------------------------------------

def test_shim_import_and_mcp_instance() -> None:
    from mcp.server.fastmcp import FastMCP
    assert isinstance(shim_mcp, FastMCP)


def test_shim_has_all_tools() -> None:
    tool_names = {t.name for t in shim_mcp._tool_manager.list_tools()}
    assert tool_names == EXPECTED_TOOLS


# ---------------------------------------------------------------------------
# E2 — list_dcc_targets
# ---------------------------------------------------------------------------

def test_list_dcc_targets_returns_targets(monkeypatch) -> None:
    targets = [
        TargetInfo(instance_id="c1", instance_name="myapp", instance_type="maya")
    ]
    client = _mock_client(list_targets=ListTargetsResponse(targets=targets))
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("list_dcc_targets", {}))
    data = result.structuredContent
    assert len(data["targets"]) == 1
    assert data["targets"][0]["instance_id"] == "c1"
    assert data["targets"][0]["instance_name"] == "myapp"
    assert data["targets"][0]["instance_type"] == "maya"
    assert json.loads(result.content[0].text) == data
    client.list_targets.assert_called_once_with(None)


def test_list_dcc_targets_returns_structured_content(monkeypatch) -> None:
    targets = [
        TargetInfo(
            instance_id="c1",
            instance_name="myapp",
            instance_type="maya",
        )
    ]
    client = _mock_client(list_targets=ListTargetsResponse(targets=targets))
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("list_dcc_targets", {}))
    assert result.structuredContent == {
        "targets": [
            {
                "instance_id": "c1",
                "instance_name": "myapp",
                "alias": None,
                "instance_type": "maya",
            }
        ]
    }


def test_list_dcc_targets_filters_by_type(monkeypatch) -> None:
    client = _mock_client(list_targets=ListTargetsResponse(targets=[]))
    _patch(monkeypatch, client)

    asyncio.run(shim_mcp.call_tool("list_dcc_targets", {"dcc_type": "maya"}))
    client.list_targets.assert_called_once_with("maya")


# ---------------------------------------------------------------------------
# E3 — exec_python
# ---------------------------------------------------------------------------

def test_exec_python_success(monkeypatch) -> None:
    result_obj = InstanceExecResult(
        execution_id="0001",
        status=InstanceExecStatus.SUCCEEDED,
    )
    client = _mock_client(execute=result_obj)
    _patch(monkeypatch, client)
    wf_id = WorkflowPersistence.create_workflow("test_wf")

    result = asyncio.run(shim_mcp.call_tool("exec_python", {
        "instance_id": "c1",
        "code": 'print("hello")',
        "workflow_id": wf_id,
    }))
    assert result.isError is False
    assert result.structuredContent["status"] == "succeeded"
    assert "stdout" not in result.structuredContent


def test_exec_python_target_not_found(monkeypatch) -> None:
    client = AsyncMock(spec=BackendClient)
    client.execute.side_effect = KeyError("unknown client: nonexistent")
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("exec_python", {
        "instance_id": "nonexistent",
        "code": "print(1)",
        "workflow_id": "dummy",
    }))
    assert result.isError is True
    assert result.structuredContent["error_code"] == "target_offline"


# ---------------------------------------------------------------------------
# E4 — start_workflow
# ---------------------------------------------------------------------------

def test_start_workflow_returns_workflow_id(monkeypatch) -> None:
    client = _mock_client(start_workflow="wf-abc123")
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("start_workflow", {"name": "my-wf"}))
    assert result.structuredContent["workflow_id"] == "wf-abc123"
    client.start_workflow.assert_called_once_with("my-wf", "")


def test_start_workflow_creates_file(monkeypatch) -> None:
    wf_id = WorkflowPersistence.create_workflow("disk-test")
    client = _mock_client(start_workflow=wf_id)
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("start_workflow", {"name": "disk-test"}))
    returned_id = result.structuredContent["workflow_id"]
    assert WorkflowPersistence.exists(returned_id)


# ---------------------------------------------------------------------------
# E5 — get_workflow_overview
# ---------------------------------------------------------------------------

def test_get_workflow_overview_existing(monkeypatch) -> None:
    overview = GetWorkflowOverviewResponse(
        workflow_id="wf-1", name="my-wf", description="desc",
        execution_count=0, created_at="2024-01-01T00:00:00+00:00",
    )
    client = _mock_client(get_workflow_overview=overview)
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("get_workflow_overview", {
        "workflow_id": "wf-1",
    }))
    assert result.isError is False
    assert result.structuredContent["workflow_id"] == "wf-1"
    assert result.structuredContent["name"] == "my-wf"


def test_get_workflow_overview_not_found(monkeypatch) -> None:
    client = AsyncMock(spec=BackendClient)
    client.get_workflow_overview.side_effect = WorkflowRecordUnavailableError("not found")
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("get_workflow_overview", {
        "workflow_id": "nonexistent",
    }))
    assert result.isError is True
    assert result.structuredContent["error_code"] == "workflow_not_found"


def test_get_workflow_overview_returns_structured_error(monkeypatch) -> None:
    client = AsyncMock(spec=BackendClient)
    client.get_workflow_overview.side_effect = WorkflowRecordUnavailableError("not found")
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("get_workflow_overview", {"workflow_id": "wf-missing"}))
    assert result.isError is True
    assert result.structuredContent == {
        "error_code": "workflow_not_found",
        "message": "not found",
    }


# ---------------------------------------------------------------------------
# E6 — get_workflow_execution
# ---------------------------------------------------------------------------

def test_get_workflow_execution_full_view(monkeypatch) -> None:
    resp = GetWorkflowExecutionResponse(
        execution_id="0001", workflow_id="wf-1", name="step",
        instance_id="c1", status="succeeded", stdout="1\n", stderr="",
        started_at="2024-01-01T00:00:00+00:00", code="print(1)",
    )
    client = _mock_client(get_workflow_execution=resp)
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("get_workflow_execution", {
        "workflow_id": "wf-1",
        "execution_id": "0001",
        "view": "full",
    }))
    assert result.structuredContent["code"] == "print(1)"


def test_get_workflow_execution_summary_no_code(monkeypatch) -> None:
    resp = GetWorkflowExecutionResponse(
        execution_id="0001", workflow_id="wf-1", name="step",
        instance_id="c1", status="succeeded", stdout="1\n", stderr="",
        started_at="2024-01-01T00:00:00+00:00",
    )
    client = _mock_client(get_workflow_execution=resp)
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("get_workflow_execution", {
        "workflow_id": "wf-1",
        "execution_id": "0001",
        "view": "summary",
    }))
    assert "code" not in result.structuredContent


def test_get_workflow_execution_not_found(monkeypatch) -> None:
    client = AsyncMock(spec=BackendClient)
    client.get_workflow_execution.side_effect = KeyError("execution not found: 9999")
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("get_workflow_execution", {
        "workflow_id": "wf-1",
        "execution_id": "9999",
    }))
    assert result.isError is True
    assert result.structuredContent["error_code"] == "execution_not_found"


# ---------------------------------------------------------------------------
# E7 — set_target_alias
# ---------------------------------------------------------------------------

def test_set_target_alias_updates_alias(monkeypatch) -> None:
    resp = SetTargetAliasResponse(success=True, instance_id="c1", alias="my-alias")
    client = _mock_client(set_alias=resp)
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("set_target_alias", {
        "instance_id": "c1",
        "alias": "my-alias",
    }))
    assert result.structuredContent["alias"] == "my-alias"
    client.set_alias.assert_called_once_with("c1", "my-alias")


def test_set_target_alias_none(monkeypatch) -> None:
    resp = SetTargetAliasResponse(success=True, instance_id="c1", alias=None)
    client = _mock_client(set_alias=resp)
    _patch(monkeypatch, client)

    result = asyncio.run(shim_mcp.call_tool("set_target_alias", {
        "instance_id": "c1",
        "alias": None,
    }))
    assert result.structuredContent["alias"] is None
