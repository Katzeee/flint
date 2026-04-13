from __future__ import annotations

from dataclasses import dataclass, field
from typing import ClassVar, List, Optional

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
