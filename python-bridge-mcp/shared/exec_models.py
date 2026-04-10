from dataclasses import dataclass
from enum import Enum
from typing import ClassVar, Optional

from .model_base import VersionedWireModel, wire_model


class ExecStatus(str, Enum):
    RUNNING = "running"
    SUCCEED = "succeed"
    FAILED = "failed"


@dataclass
class ExecWireModel(VersionedWireModel):
    PROTOCOL_VERSION: ClassVar[int] = 2


@wire_model
@dataclass
class ExecRequest(ExecWireModel):
    request_id: str
    code: str
    workflow_id: str


@wire_model
@dataclass
class ExecResult(ExecWireModel):
    request_id: str
    status: ExecStatus
    stdout: str
    stderr: str
    traceback: Optional[str] = None
    error: Optional[str] = None
