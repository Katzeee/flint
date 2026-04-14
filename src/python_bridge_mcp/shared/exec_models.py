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
    BUSY = "busy"


@dataclass
class ExecWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 2


@wire_model
@dataclass
class ExecRequest(ExecWireModel):
    execution_id: str
    code: str
    workflow_id: str
    execution_name: Optional[str] = None


@wire_model
@dataclass
class ExecResult(ExecWireModel):
    execution_id: str
    status: ExecStatus
    stdout: str
    stderr: str
    traceback: Optional[str] = None
    error: Optional[str] = None
