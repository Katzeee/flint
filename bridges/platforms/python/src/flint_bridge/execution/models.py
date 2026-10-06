from dataclasses import dataclass
from enum import Enum
from typing import Optional


class InstanceExecStatus(str, Enum):
    RUNNING = "running"
    SUCCEEDED = "succeeded"
    FAILED = "failed"


@dataclass
class InstanceExecResult:
    execution_id: str
    status: InstanceExecStatus
    traceback: Optional[str] = None
    error: Optional[str] = None
