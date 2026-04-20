from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, ClassVar, Dict, List, Optional

from ..shared.model_base import BaseModel, VersionedWireModel, wire_model


@dataclass
class ControlWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 1


@dataclass
class TargetInfo(BaseModel):
    instance_id: str
    instance_name: str
    exec_host: str
    exec_port: int
    alias: Optional[str] = None
    instance_type: str = ""


@dataclass
class TargetSummary(BaseModel):
    instance_id: str
    exec_count: int
    active_count: int
    latest_status: Optional[str] = None


@wire_model
@dataclass
class ListTargetsRequest(ControlWireModel):
    instance_type: Optional[str] = None


@wire_model
@dataclass
class ListTargetsResponse(ControlWireModel):
    targets: List[TargetInfo] = field(default_factory=list)


@wire_model
@dataclass
class StartWorkflowRequest(ControlWireModel):
    name: str = ""
    description: str = ""


@wire_model
@dataclass
class StartWorkflowResponse(ControlWireModel):
    workflow_id: str = ""


@wire_model
@dataclass
class GetWorkflowOverviewRequest(ControlWireModel):
    workflow_id: str = ""


@wire_model
@dataclass
class GetWorkflowOverviewResponse(ControlWireModel):
    workflow_id: str = ""
    name: str = ""
    execution_count: int = 0
    created_at: str = ""
    description: str = ""
    instance_ids: List[str] = field(default_factory=list)
    target_summaries: List[TargetSummary] = field(default_factory=list)


@wire_model
@dataclass
class GetWorkflowExecutionRequest(ControlWireModel):
    workflow_id: str = ""
    execution_id: str = ""
    view: str = "summary"


@wire_model
@dataclass
class GetWorkflowExecutionResponse(ControlWireModel):
    execution_id: str = ""
    workflow_id: str = ""
    name: str = ""
    instance_id: str = ""
    status: str = ""
    stdout: str = ""
    stderr: str = ""
    started_at: str = ""
    finished_at: Optional[str] = None
    traceback: Optional[str] = None
    error: Optional[str] = None
    updated_at: Optional[str] = None
    code: Optional[str] = None


@wire_model
@dataclass
class SetTargetAliasRequest(ControlWireModel):
    instance_id: str = ""
    alias: Optional[str] = None


@wire_model
@dataclass
class SetTargetAliasResponse(ControlWireModel):
    success: bool = False
    instance_id: str = ""
    alias: Optional[str] = None


@wire_model
@dataclass
class ControlExecuteRequest(ControlWireModel):
    instance_id: str = ""
    code: str = ""
    workflow_id: str = ""
    name: str = ""


@wire_model
@dataclass
class ErrorResponse(ControlWireModel):
    error_code: str = ""
    message: str = ""


@wire_model
@dataclass
class PingRequest(ControlWireModel):
    pass


@wire_model
@dataclass
class PingResponse(ControlWireModel):
    ok: bool = True
    service: str = "python-bridge-backend"
    ready: bool = True
