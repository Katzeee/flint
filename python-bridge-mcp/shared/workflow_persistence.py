import json
import time
from typing import Optional

from .exec_models import ExecStatus
from .file_writer import FileWriter
from .workflow_models import ExecEntry, WorkflowRecord


class WorkflowPersistence:

    @staticmethod
    def create_workflow(
        workflow_file_path: str,
        workflow_id: str,
        name: str,
        description: str = "",
    ) -> None:
        """Create a new workflow file with metadata."""
        with FileWriter.locked(workflow_file_path) as f:
            record = WorkflowRecord(
                workflow_id=workflow_id,
                name=name,
                description=description,
                created_at=time.time(),
            )
            f.write(json.dumps(record.to_dict(), indent=2))

    @staticmethod
    def exists(workflow_file_path: str) -> bool:
        with FileWriter.locked(workflow_file_path) as f:
            return f.exists()

    @staticmethod
    def append_running_execution(
        workflow_file_path: str,
        request_id: str,
        workflow_id: str,
        instance_id: str,
        code: str,
    ) -> None:
        """Server calls this before sending ExecRequest. Appends RUNNING entry."""
        with FileWriter.locked(workflow_file_path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            record.execs.append(ExecEntry(
                request_id=request_id,
                workflow_id=workflow_id,
                instance_id=instance_id,
                code=code,
                status=ExecStatus.RUNNING,
                stdout="",
                stderr="",
                started_at=time.time(),
            ))
            if instance_id not in record.instance_ids:
                record.instance_ids.append(instance_id)
            f.write(json.dumps(record.to_dict(), indent=2))

    @staticmethod
    def update_execution_output(
        workflow_file_path: str,
        request_id: str,
        stdout: str,
        stderr: str,
    ) -> None:
        """Client calls this periodically. Updates stdout/stderr for RUNNING entry."""
        with FileWriter.locked(workflow_file_path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            for entry in record.execs:
                if entry.request_id == request_id:
                    entry.stdout = stdout
                    entry.stderr = stderr
                    break
            f.write(json.dumps(record.to_dict(), indent=2))

    @staticmethod
    def update_execution_result(
        workflow_file_path: str,
        request_id: str,
        status: ExecStatus,
        stdout: str,
        stderr: str,
        finished_at: float,
        traceback: Optional[str] = None,
        error: Optional[str] = None,
    ) -> None:
        """Server calls this on completion. Writes final status + output."""
        with FileWriter.locked(workflow_file_path) as f:
            record = WorkflowRecord.from_dict(json.loads(f.read()))
            for entry in record.execs:
                if entry.request_id == request_id:
                    entry.status = status
                    entry.stdout = stdout
                    entry.stderr = stderr
                    entry.finished_at = finished_at
                    entry.traceback = traceback
                    entry.error = error
                    break
            f.write(json.dumps(record.to_dict(), indent=2))
