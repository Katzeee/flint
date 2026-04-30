from dataclasses import dataclass
from enum import Enum
from typing import ClassVar, Optional

from .model_base import VersionedWireModel, wire_model


class InstanceExecStatus(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    SUCCEEDED = "succeeded"
    FAILED = "failed"


class InstanceExecError(str, Enum):
    BUSY              = "busy"
    TARGET_OFFLINE    = "target_offline"
    CONNECTION_FAILED = "connection_failed"
    PROTOCOL_ERROR    = "protocol_error"
    EXECUTION_TIMEOUT = "execution_timeout"


class InstanceControlError(str, Enum):
    PROTOCOL_ERROR = "protocol_error"
    ALREADY_REGISTERED = "already_registered"
    NOT_REGISTERED = "not_registered"
    UNEXPECTED_MESSAGE_TYPE = "unexpected_message_type"


@dataclass
class InstanceControlWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 3


@wire_model
@dataclass
class InstanceRegister(InstanceControlWireModel):
    pid: int
    instance_id: str
    instance_name: str
    alias: Optional[str] = None
    instance_type: str = ""


@wire_model
@dataclass
class InstanceHeartbeat(InstanceControlWireModel):
    instance_id: str


@wire_model
@dataclass
class InstanceAck(InstanceControlWireModel):
    success: bool
    error_code: Optional[InstanceControlError] = None
    message: str = ""


@wire_model
@dataclass
class InstanceExecRequest(InstanceControlWireModel):
    execution_id: str
    code: str
    workflow_id: str
    execution_name: Optional[str] = None
    request_id: Optional[str] = None


@wire_model
@dataclass
class InstanceSetAliasRequest(InstanceControlWireModel):
    alias: Optional[str] = None
    request_id: Optional[str] = None


@wire_model
@dataclass
class InstanceSetAliasResult(InstanceControlWireModel):
    success: bool = False
    alias: Optional[str] = None
    request_id: Optional[str] = None


@wire_model
@dataclass
class InstanceExecOutputUpdate(InstanceControlWireModel):
    execution_id: str
    workflow_id: str
    sequence: int
    stdout_delta: str = ""
    stderr_delta: str = ""
    request_id: Optional[str] = None


@wire_model
@dataclass
class InstanceExecResult(InstanceControlWireModel):
    execution_id: str
    status: InstanceExecStatus
    traceback: Optional[str] = None
    error: Optional[str] = None
    request_id: Optional[str] = None
