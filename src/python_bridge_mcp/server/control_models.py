from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import ClassVar, List, Optional

from ..shared.model_base import BaseModel, VersionedWireModel, wire_model


@dataclass
class ControlWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 1


class ControlError(str, Enum):
    PROTOCOL_ERROR = "protocol_error"
    INTERNAL_ERROR = "internal_error"
    UNKNOWN_CLIENT = "unknown_client"
    UNKNOWN_REQUEST = "unknown_request"
    WORKFLOW_NOT_FOUND = "workflow_not_found"
    EXECUTION_NOT_FOUND = "execution_not_found"
    INSTANCE_OFFLINE = "instance_offline"


@dataclass
class InstanceInfo(BaseModel):
    instance_id: str
    instance_name: str
    alias: Optional[str] = None
    instance_type: str = ""


@wire_model
@dataclass
class ListInstancesRequest(ControlWireModel):
    instance_type: Optional[str] = None


@wire_model
@dataclass
class ListInstancesResponse(ControlWireModel):
    instances: List[InstanceInfo] = field(default_factory=list)


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
class ControlExecuteRequest(ControlWireModel):
    instance_id: str = ""
    code: str = ""
    workflow_id: str = ""
    name: str = ""


@wire_model
@dataclass
class ErrorResponse(ControlWireModel):
    error_code: ControlError = ControlError.INTERNAL_ERROR
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
