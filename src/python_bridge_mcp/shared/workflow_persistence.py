import json
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Optional

from platformdirs import user_data_dir

from .exec_models import ExecStatus
from .file_writer import FileWriter
from .workflow_models import ExecEntry, WorkflowRecord


class WorkflowRecordUnavailableError(Exception):
    """Raised when a requested workflow record file does not exist on disk."""


class WorkflowPersistence:

    BASE_DIR: Path = Path(user_data_dir("python-bridge-mcp")) / "workflows"

    @staticmethod
    def _path_for(workflow_id: str) -> str:
        """Return the file-system path for a workflow ID without checking existence."""
        return str(WorkflowPersistence.BASE_DIR / f"{workflow_id}.json")

    @staticmethod
    def resolve(workflow_id: str) -> str:
        """Return the file-system path for a workflow ID.

        Raises WorkflowRecordUnavailableError if the file does not exist.
        """
        path = WorkflowPersistence._path_for(workflow_id)
        if not Path(path).exists():
            raise WorkflowRecordUnavailableError(f"workflow not found: {workflow_id}")
        return path

    @staticmethod
    def create_workflow(name: str, description: str = "") -> str:
        """Create a new workflow file. Returns workflow_id."""
        now = datetime.now(timezone.utc)
        ts = now.strftime("%Y%m%d_%H%M%S")
        short_id = uuid.uuid4().hex[:8]
        workflow_id = f"{name}_{ts}_{short_id}"
        path = WorkflowPersistence._path_for(workflow_id)
        with FileWriter.locked(path) as f:
            record = WorkflowRecord(
                workflow_id=workflow_id,
                name=name,
                description=description,
                created_at=now.isoformat(),
            )
            f.write(json.dumps(record.to_dict(), indent=2))
        return workflow_id

    @staticmethod
    def exists(workflow_id: str) -> bool:
        return Path(WorkflowPersistence._path_for(workflow_id)).exists()

    @staticmethod
    def load(workflow_id: str) -> "WorkflowRecord":
        """Read and return the WorkflowRecord for workflow_id.

        Raises WorkflowRecordUnavailableError if the file does not exist.
        """
        path = WorkflowPersistence.resolve(workflow_id)
        with open(path) as f:
            return WorkflowRecord.from_dict(json.loads(f.read()))

    @staticmethod
    def append_running_execution(
        workflow_id: str,
        name: str,
        instance_id: str,
        code: str,
        request_id: Optional[str] = None,
    ) -> str:
        """Server calls this before sending ExecRequest. Appends RUNNING entry.

        Returns the generated execution_id.
        """
        path = WorkflowPersistence.resolve(workflow_id)
        with FileWriter.locked(path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            record.latest_execution_id += 1
            execution_id = f"{record.latest_execution_id:04d}"
            record.execs.append(ExecEntry(
                execution_id=execution_id,
                name=name,
                workflow_id=workflow_id,
                instance_id=instance_id,
                code=code,
                status=ExecStatus.RUNNING,
                stdout="",
                stderr="",
                started_at=datetime.now(timezone.utc).isoformat(),
                request_id=request_id,
            ))
            record.execution_count = len(record.execs)
            if instance_id not in record.instance_ids:
                record.instance_ids.append(instance_id)
            f.write(json.dumps(record.to_dict(), indent=2))
        return execution_id

    @staticmethod
    def remove_execution(workflow_id: str, execution_id: str) -> None:
        """Remove an execution entry from the workflow file."""
        path = WorkflowPersistence.resolve(workflow_id)
        with FileWriter.locked(path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            record.execs = [e for e in record.execs if e.execution_id != execution_id]
            record.execution_count = len(record.execs)
            f.write(json.dumps(record.to_dict(), indent=2))

    @staticmethod
    def update_execution_output(
        workflow_id: str,
        execution_id: str,
        stdout: str,
        stderr: str,
    ) -> None:
        """Client calls this periodically. Updates stdout/stderr for RUNNING entry."""
        path = WorkflowPersistence.resolve(workflow_id)
        with FileWriter.locked(path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            for entry in record.execs:
                if entry.execution_id == execution_id:
                    entry.stdout = stdout
                    entry.stderr = stderr
                    entry.updated_at = datetime.now(timezone.utc).isoformat()
                    break
            f.write(json.dumps(record.to_dict(), indent=2))

    @staticmethod
    def update_execution_result(
        workflow_id: str,
        execution_id: str,
        status: ExecStatus,
        stdout: str,
        stderr: str,
        finished_at: str,
        traceback: Optional[str] = None,
        error: Optional[str] = None,
    ) -> None:
        """Server calls this on completion. Writes final status + output."""
        path = WorkflowPersistence.resolve(workflow_id)
        with FileWriter.locked(path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            for entry in record.execs:
                if entry.execution_id == execution_id:
                    entry.status = status
                    entry.stdout = stdout
                    entry.stderr = stderr
                    entry.finished_at = finished_at
                    entry.traceback = traceback
                    entry.error = error
                    entry.updated_at = finished_at
                    break
            f.write(json.dumps(record.to_dict(), indent=2))

    @staticmethod
    def get_target_summaries(record: "WorkflowRecord") -> Dict[str, Dict[str, Any]]:
        """Return per-instance_id execution summary for a loaded workflow record.

        Returns a dict keyed by instance_id, each value containing:
            exec_count    - total executions on that target
            active_count  - currently RUNNING executions
            latest_status - status of the most recent execution (str or None)
        """
        summaries = {}  # type: Dict[str, Dict[str, Any]]
        for entry in record.execs:
            iid = entry.instance_id
            if iid not in summaries:
                summaries[iid] = {"exec_count": 0, "active_count": 0, "latest_status": None}
            summaries[iid]["exec_count"] += 1
            if entry.status == ExecStatus.RUNNING:
                summaries[iid]["active_count"] += 1
            summaries[iid]["latest_status"] = entry.status.value
        return summaries
