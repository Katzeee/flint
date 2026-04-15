from dataclasses import dataclass, field
from typing import List, Optional

from .exec_models import ExecStatus
from .model_base import BaseModel


@dataclass
class ExecEntry(BaseModel):
    execution_id: str
    name: str
    workflow_id: str
    instance_id: str
    code: str
    status: ExecStatus
    stdout: str
    stderr: str
    started_at: str
    finished_at: Optional[str] = None
    traceback: Optional[str] = None
    error: Optional[str] = None
    updated_at: Optional[str] = None
    request_id: Optional[str] = None


@dataclass
class WorkflowRecord(BaseModel):
    workflow_id: str
    name: str
    description: str
    created_at: str
    schema_version: int = 1
    latest_execution_id: int = 0
    execution_count: int = 0
    instance_ids: List[str] = field(default_factory=list)
    execs: List[ExecEntry] = field(default_factory=list)
