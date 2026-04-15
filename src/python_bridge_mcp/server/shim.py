"""MCP shim — exposes ControlServer methods as FastMCP tools."""

import json
from typing import Optional

from mcp.server.fastmcp import FastMCP
from mcp.server.fastmcp.exceptions import ToolError

from .app import App
from .control_server import ControlServer
from ..shared.exec_models import ExecError, ExecResult, ExecStatus
from ..shared.workflow_persistence import WorkflowRecordUnavailableError

mcp = FastMCP("python-bridge-mcp")

_app: Optional[App] = None


def _get_control() -> ControlServer:
    """Return the shared ControlServer instance, starting the App if needed."""
    global _app
    if _app is None:
        _app = App()
    return _app.control


# ------------------------------------------------------------------
# Tools
# ------------------------------------------------------------------


@mcp.tool()
def list_dcc_targets(dcc_type: Optional[str] = None) -> str:
    """List currently registered DCC targets.

    Args:
        dcc_type: Optional filter by instance type (e.g. "maya", "nuke").
    """
    control = _get_control()
    response = control.list_targets(dcc_type)
    return json.dumps([t.to_dict() for t in response.targets])


@mcp.tool()
async def exec_python(instance_id: str, code: str, workflow_id: str, name: str = "") -> str:
    """Execute Python code on a remote DCC target.

    Args:
        instance_id: Target instance to execute on.
        code: Python code to execute.
        workflow_id: Workflow to record execution under.
        name: Optional execution name.
    """
    control = _get_control()
    try:
        result = await control.execute(instance_id, code, workflow_id, name)
    except KeyError:
        result = ExecResult(
            execution_id="",
            status=ExecStatus.FAILED,
            stdout="",
            stderr="",
            error=ExecError.TARGET_OFFLINE,
        )
    return json.dumps(result.to_dict(exclude_none=True))


@mcp.tool()
def start_workflow(name: str, description: str = "") -> str:
    """Create a new workflow and return its ID.

    Args:
        name: Workflow name.
        description: Optional description.
    """
    control = _get_control()
    workflow_id = control.start_workflow(name, description)
    return json.dumps({"workflow_id": workflow_id})


@mcp.tool()
def get_workflow_overview(workflow_id: str) -> str:
    """Get overview of a workflow (name, description, execution count, etc.).

    Args:
        workflow_id: The workflow ID to look up.
    """
    control = _get_control()
    try:
        overview = control.get_workflow_overview(workflow_id)
    except WorkflowRecordUnavailableError as exc:
        raise ToolError(str(exc)) from exc
    return json.dumps(overview.to_dict(exclude_none=True))


@mcp.tool()
def get_workflow_execution(workflow_id: str, execution_id: str, view: str = "summary") -> str:
    """Get details of a specific execution within a workflow.

    Args:
        workflow_id: The workflow ID.
        execution_id: The execution ID within the workflow.
        view: "summary" (default) omits code, "full" includes code.
    """
    control = _get_control()
    try:
        response = control.get_workflow_execution(workflow_id, execution_id, view)
    except (KeyError, WorkflowRecordUnavailableError) as exc:
        raise ToolError(str(exc)) from exc
    return json.dumps(response.to_dict(exclude_none=True))


@mcp.tool()
def set_target_alias(instance_id: str, alias: Optional[str] = None) -> str:
    """Set or clear the alias for a registered target.

    Args:
        instance_id: Target instance to update.
        alias: New alias value, or None/empty to clear.
    """
    control = _get_control()
    response = control.set_alias(instance_id, alias)
    return json.dumps(response.to_dict())
