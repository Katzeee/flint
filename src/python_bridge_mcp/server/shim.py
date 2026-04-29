"""MCP shim — exposes BackendClient methods as FastMCP tools."""

import asyncio
import json
import logging
from typing import Optional

from mcp.server.fastmcp import FastMCP
from mcp.types import CallToolResult, TextContent

from .backend_client import BackendClient, BackendError
from .control_models import ControlError
from .launcher import BackendLauncher
from ..shared.workflow_persistence import WorkflowRecordUnavailableError

mcp = FastMCP("python-bridge-mcp")
log = logging.getLogger(__name__)

_backend_client: Optional[BackendClient] = None
_client_lock: Optional[asyncio.Lock] = None


async def _get_backend_client() -> BackendClient:
    global _backend_client, _client_lock
    if _client_lock is None:
        _client_lock = asyncio.Lock()
    async with _client_lock:
        if _backend_client is None:
            await BackendLauncher().ensure_running()
            _backend_client = BackendClient()
    return _backend_client


def _tool_ok(payload: dict) -> CallToolResult:
    return CallToolResult(
        content=[TextContent(type="text", text=json.dumps(payload, ensure_ascii=False, indent=2))],
        structuredContent=payload,
        isError=False,
    )


def _tool_error(error_code: ControlError, message: str) -> CallToolResult:
    return CallToolResult(
        content=[TextContent(type="text", text=message)],
        structuredContent={"error_code": error_code.value, "message": message},
        isError=True,
    )


# ------------------------------------------------------------------
# Tools
# ------------------------------------------------------------------


@mcp.tool()
async def list_dcc_targets(dcc_type: Optional[str] = None):
    """List currently registered DCC targets.

    Args:
        dcc_type: Optional filter by instance type (e.g. "maya", "nuke").
    """
    client = await _get_backend_client()
    response = await client.list_targets(dcc_type)
    return _tool_ok({"targets": [t.to_dict() for t in response.targets]})


@mcp.tool()
async def exec_python(instance_id: str, code: str, workflow_id: str, name: str = ""):
    """Execute Python code on a remote DCC target.

    Args:
        instance_id: Target instance to execute on.
        code: Python code to execute.
        workflow_id: Workflow to record execution under.
        name: Optional execution name.
    """
    client = await _get_backend_client()
    try:
        result = await client.execute(instance_id, code, workflow_id, name)
    except KeyError as exc:
        return _tool_error(ControlError.TARGET_OFFLINE, str(exc))
    return _tool_ok(result.to_dict(exclude_none=True))


@mcp.tool()
async def start_workflow(name: str, description: str = ""):
    """Create a new workflow and return its ID.

    Args:
        name: Workflow name.
        description: Optional description.
    """
    client = await _get_backend_client()
    workflow_id = await client.start_workflow(name, description)
    return _tool_ok({"workflow_id": workflow_id})


@mcp.tool()
async def get_workflow_overview(workflow_id: str):
    """Get overview of a workflow (name, description, execution count, etc.).

    Args:
        workflow_id: The workflow ID to look up.
    """
    client = await _get_backend_client()
    try:
        overview = await client.get_workflow_overview(workflow_id)
    except WorkflowRecordUnavailableError as exc:
        return _tool_error(ControlError.WORKFLOW_NOT_FOUND, str(exc))
    return _tool_ok(overview.to_dict(exclude_none=True))


@mcp.tool()
async def get_workflow_execution(workflow_id: str, execution_id: str, view: str = "summary"):
    """Get details of a specific execution within a workflow.

    Args:
        workflow_id: The workflow ID.
        execution_id: The execution ID within the workflow.
        view: "summary" (default) omits code, "full" includes code.
    """
    client = await _get_backend_client()
    try:
        response = await client.get_workflow_execution(workflow_id, execution_id, view)
    except WorkflowRecordUnavailableError as exc:
        return _tool_error(ControlError.WORKFLOW_NOT_FOUND, str(exc))
    except KeyError as exc:
        return _tool_error(ControlError.EXECUTION_NOT_FOUND, str(exc))
    return _tool_ok(response.to_dict(exclude_none=True))


@mcp.tool()
async def set_target_alias(instance_id: str, alias: Optional[str] = None):
    """Set or clear the alias for a registered target.

    Args:
        instance_id: Target instance to update.
        alias: New alias value, or None/empty to clear.
    """
    client = await _get_backend_client()
    try:
        response = await client.set_alias(instance_id, alias)
    except KeyError as exc:
        return _tool_error(ControlError.TARGET_OFFLINE, str(exc))
    except BackendError as exc:
        return _tool_error(exc.error_code, str(exc))
    return _tool_ok(response.to_dict())


if __name__ == "__main__":
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(name)s] %(levelname)s %(message)s",
    )
    asyncio.run(BackendLauncher().ensure_running())
    mcp.run()
