from dataclasses import dataclass
from enum import Enum
from typing import ClassVar, Optional

from .model_base import VersionedWireModel, wire_model


class ExecStatus(str, Enum):
    PENDING = "pending"
    RUNNING = "running"
    SUCCEEDED = "succeeded"
    FAILED = "failed"


class ExecError(str, Enum):
    BUSY              = "busy"
    TARGET_OFFLINE    = "target_offline"
    CONNECTION_FAILED = "connection_failed"
    PROTOCOL_ERROR    = "protocol_error"
    EXECUTION_TIMEOUT = "execution_timeout"


@dataclass
class ExecWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 3


@wire_model
@dataclass
class ExecRequest(ExecWireModel):
    execution_id: str
    code: str
    workflow_id: str
    execution_name: Optional[str] = None
    request_id: Optional[str] = None


@wire_model
@dataclass
class SetAliasRequest(ExecWireModel):
    alias: Optional[str] = None
    request_id: Optional[str] = None


@wire_model
@dataclass
class SetAliasResult(ExecWireModel):
    success: bool = False
    alias: Optional[str] = None
    request_id: Optional[str] = None


@wire_model
@dataclass
class ExecOutputUpdate(ExecWireModel):
    execution_id: str
    workflow_id: str
    sequence: int
    stdout_delta: str = ""
    stderr_delta: str = ""
    request_id: Optional[str] = None


@wire_model
@dataclass
class ExecResult(ExecWireModel):
    execution_id: str
    status: ExecStatus
    traceback: Optional[str] = None
    error: Optional[str] = None
    request_id: Optional[str] = None
