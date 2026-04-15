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
    PROTOCOL_VERSION: ClassVar[int] = 2


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
class ExecResult(ExecWireModel):
    execution_id: str
    status: ExecStatus
    stdout: Optional[str] = None
    stderr: Optional[str] = None
    traceback: Optional[str] = None
    error: Optional[str] = None
    request_id: Optional[str] = None

    def __post_init__(self) -> None:
        if self.status == ExecStatus.RUNNING:
            self.stdout = None
            self.stderr = None
            self.traceback = None
        elif self.status in (ExecStatus.SUCCEEDED, ExecStatus.FAILED):
            if not isinstance(self.stdout, str):
                raise ValueError(
                    f"ExecResult with status={self.status!r} requires stdout as str, got {self.stdout!r}"
                )
            if not isinstance(self.stderr, str):
                raise ValueError(
                    f"ExecResult with status={self.status!r} requires stderr as str, got {self.stderr!r}"
                )
