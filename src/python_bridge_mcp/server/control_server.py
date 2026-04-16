import asyncio
from datetime import datetime, timezone
from typing import Dict, Optional
from uuid import uuid4

from .registry import ClientEntry, Registry
from .control_models import (
    GetWorkflowExecutionResponse,
    GetWorkflowOverviewResponse,
    ListTargetsResponse,
    SetTargetAliasResponse,
    TargetInfo,
    TargetSummary,
)
from ..shared.exec_models import ExecError, ExecRequest, ExecResult, ExecStatus
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError
from ..shared.workflow_persistence import WorkflowPersistence


class ControlServer:
    DEFAULT_CONNECT_TIMEOUT: float = 10.0
    DEFAULT_EARLY_RETURN_WINDOW: float = 5.0

    def __init__(self, discovery: Registry) -> None:
        self._discovery = discovery

    def list_clients(self, instance_type: Optional[str] = None) -> Dict[str, ClientEntry]:
        return self._discovery.list_clients(instance_type)

    def list_targets(self, instance_type: Optional[str] = None) -> ListTargetsResponse:
        clients = self._discovery.list_clients(instance_type)
        targets = [
            TargetInfo(
                instance_id=e.instance_id,
                instance_name=e.instance_name,
                exec_host=e.exec_host,
                exec_port=e.exec_port,
                alias=e.alias,
                instance_type=e.instance_type,
            )
            for e in clients.values()
        ]
        return ListTargetsResponse(targets=targets)

    def set_alias(self, instance_id: str, alias: Optional[str]) -> SetTargetAliasResponse:
        self._discovery.set_alias(instance_id, alias)
        entry = self._discovery.get_client(instance_id)
        return SetTargetAliasResponse(
            success=True,
            instance_id=instance_id,
            alias=entry.alias if entry is not None else None,
        )

    def get_workflow_overview(self, workflow_id: str) -> GetWorkflowOverviewResponse:
        record = WorkflowPersistence.load(workflow_id)
        raw_summaries = WorkflowPersistence.get_target_summaries(record)
        target_summaries = [
            TargetSummary(
                instance_id=iid,
                exec_count=s["exec_count"],
                active_count=s["active_count"],
                latest_status=s["latest_status"],
            )
            for iid, s in raw_summaries.items()
        ]
        return GetWorkflowOverviewResponse(
            workflow_id=record.workflow_id,
            name=record.name,
            description=record.description,
            execution_count=record.execution_count,
            created_at=record.created_at,
            instance_ids=list(record.instance_ids),
            target_summaries=target_summaries,
        )

    def get_workflow_execution(
        self,
        workflow_id: str,
        execution_id: str,
        view: str = "summary",
    ) -> GetWorkflowExecutionResponse:
        record = WorkflowPersistence.load(workflow_id)
        for entry in record.execs:
            if entry.execution_id == execution_id:
                return GetWorkflowExecutionResponse(
                    execution_id=entry.execution_id,
                    workflow_id=entry.workflow_id,
                    name=entry.name,
                    instance_id=entry.instance_id,
                    status=entry.status.value,
                    stdout=entry.stdout,
                    stderr=entry.stderr,
                    started_at=entry.started_at,
                    finished_at=entry.finished_at,
                    traceback=entry.traceback,
                    error=entry.error,
                    updated_at=entry.updated_at,
                    code=entry.code if view == "full" else None,
                )
        raise KeyError(f"execution not found: {execution_id}")

    def start_workflow(self, name: str, description: str = "") -> str:
        return WorkflowPersistence.create_workflow(name, description)

    async def execute(
        self,
        instance_id: str,
        code: str,
        workflow_id: str,
        name: str = "",
        *,
        connect_timeout: float = DEFAULT_CONNECT_TIMEOUT,
        early_return_window: float = DEFAULT_EARLY_RETURN_WINDOW,
    ) -> ExecResult:
        entry = self._discovery.get_client(instance_id)
        if entry is None:
            raise KeyError(f"unknown client: {instance_id}")

        request_id = uuid4().hex

        execution_id = WorkflowPersistence.append_running_execution(
            workflow_id,
            name,
            instance_id,
            code,
            request_id=request_id,
        )

        try:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(
                    entry.exec_host,
                    entry.exec_port,
                    limit=AsyncJsonLineCodec.READER_LIMIT,
                ),
                timeout=connect_timeout,
            )
        except (asyncio.TimeoutError, OSError):
            ControlServer._fail_execution(workflow_id, execution_id, ExecError.CONNECTION_FAILED)
            return ExecResult(
                execution_id=execution_id,
                status=ExecStatus.FAILED,
                stdout="",
                stderr="",
                error=ExecError.CONNECTION_FAILED,
                request_id=request_id,
            )

        req = ExecRequest(
            execution_id=execution_id,
            code=code,
            workflow_id=workflow_id,
            execution_name=name or None,
            request_id=request_id,
        )
        try:
            await AsyncJsonLineCodec.send(writer, req.to_dict())
        except Exception:
            writer.close()
            await writer.wait_closed()
            ControlServer._fail_execution(workflow_id, execution_id, ExecError.CONNECTION_FAILED)
            return ExecResult(
                execution_id=execution_id,
                status=ExecStatus.FAILED,
                stdout="",
                stderr="",
                error=ExecError.CONNECTION_FAILED,
                request_id=request_id,
            )

        task = asyncio.create_task(
            self._receive_result(reader, writer, workflow_id, execution_id)
        )
        task.add_done_callback(ControlServer._consume_task_exception)

        try:
            return await asyncio.wait_for(asyncio.shield(task), timeout=early_return_window)
        except asyncio.TimeoutError:
            return ExecResult(
                execution_id=execution_id,
                status=ExecStatus.RUNNING,
                request_id=request_id,
            )

    @staticmethod
    def _fail_execution(workflow_id: str, execution_id: str, error: ExecError) -> None:
        WorkflowPersistence.update_execution_result(
            workflow_id,
            execution_id,
            ExecStatus.FAILED,
            "", "",
            datetime.now(timezone.utc).isoformat(),
            error=error,
        )

    @staticmethod
    def _consume_task_exception(task: asyncio.Task) -> None:
        """Retrieve exception (if any) so asyncio does not log it as unhandled."""
        if not task.cancelled():
            task.exception()

    @staticmethod
    async def _receive_result(
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        workflow_id: str,
        execution_id: str,
    ) -> ExecResult:
        try:
            data = await AsyncJsonLineCodec.recv(reader)
            result = VersionedWireModel.parse_versioned(data)
            if not isinstance(result, ExecResult):
                raise WireModelError(f"unexpected response: {type(result).__name__}")
            if result.error == ExecError.BUSY:
                WorkflowPersistence.remove_execution(workflow_id, execution_id)
            else:
                WorkflowPersistence.update_execution_result(
                    workflow_id,
                    execution_id,
                    result.status,
                    result.stdout or "",
                    result.stderr or "",
                    datetime.now(timezone.utc).isoformat(),
                    result.traceback,
                    result.error,
                )
            return result
        except WireModelError:
            ControlServer._fail_execution(workflow_id, execution_id, ExecError.PROTOCOL_ERROR)
            raise
        except Exception:
            ControlServer._fail_execution(workflow_id, execution_id, ExecError.CONNECTION_FAILED)
            raise
        finally:
            writer.close()
            await writer.wait_closed()
