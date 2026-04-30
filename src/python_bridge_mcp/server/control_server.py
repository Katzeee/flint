import asyncio
import logging
from datetime import datetime, timezone
from typing import Optional
from uuid import uuid4

from ..shared.instance_control_models import (
    InstanceExecError,
    InstanceExecOutputUpdate,
    InstanceExecRequest,
    InstanceExecResult,
    InstanceExecStatus,
    InstanceSetAliasRequest,
    InstanceSetAliasResult,
)
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError
from ..shared.workflow_persistence import WorkflowPersistence, WorkflowRecordUnavailableError
from .control_models import (
    ControlError,
    ControlExecuteRequest,
    ErrorResponse,
    GetWorkflowExecutionRequest,
    GetWorkflowExecutionResponse,
    GetWorkflowOverviewRequest,
    GetWorkflowOverviewResponse,
    ListTargetsRequest,
    ListTargetsResponse,
    PingRequest,
    PingResponse,
    SetTargetAliasRequest,
    SetTargetAliasResponse,
    StartWorkflowRequest,
    StartWorkflowResponse,
    TargetInfo,
    TargetSummary,
)
from .registry import Registry

log = logging.getLogger(__name__)


class ControlServer:
    DEFAULT_HOST: str = "localhost"
    DEFAULT_PORT: int = 6322
    DEFAULT_CONNECT_TIMEOUT: float = 10.0
    DEFAULT_EARLY_RETURN_WINDOW: float = 5.0
    BACKGROUND_EXEC_TIMEOUT: float = 600.0

    def __init__(
        self,
        discovery: Registry,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
    ) -> None:
        self._discovery = discovery
        self._host = host
        self._port = port
        self._server: Optional[asyncio.AbstractServer] = None
        self._discovery.set_output_update_handler(self.handle_output_update)

    async def run(self) -> None:
        self._server = await asyncio.start_server(
            self._handle_connection,
            self._host,
            self._port,
            limit=AsyncJsonLineCodec.READER_LIMIT,
        )
        log.info("ControlServer listening on %s:%d", self._host, self._port)
        async with self._server:
            await self._server.serve_forever()

    def stop(self) -> None:
        if self._server is not None:
            self._server.close()

    async def _handle_connection(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        response: Optional[VersionedWireModel] = None
        try:
            data = await AsyncJsonLineCodec.recv(reader)
            request = VersionedWireModel.parse_versioned(data)
            response = await self._dispatch(request)
        except ConnectionError:
            pass
        except WireModelError as exc:
            response = ErrorResponse(error_code=ControlError.PROTOCOL_ERROR, message=str(exc))
        except Exception as exc:
            response = ErrorResponse(error_code=ControlError.INTERNAL_ERROR, message=str(exc))
        finally:
            if response is not None:
                await AsyncJsonLineCodec.send(writer, response.to_dict())
            writer.close()

    async def _dispatch(self, request: VersionedWireModel) -> VersionedWireModel:
        if isinstance(request, ListTargetsRequest):
            return self.list_targets(request.instance_type)

        if isinstance(request, StartWorkflowRequest):
            wf_id = self.start_workflow(request.name, request.description)
            return StartWorkflowResponse(workflow_id=wf_id)

        if isinstance(request, ControlExecuteRequest):
            try:
                return await self.execute(request.instance_id, request.code, request.workflow_id, request.name)
            except KeyError as exc:
                return ErrorResponse(error_code=ControlError.UNKNOWN_CLIENT, message=str(exc))

        if isinstance(request, GetWorkflowOverviewRequest):
            try:
                return self.get_workflow_overview(request.workflow_id)
            except WorkflowRecordUnavailableError as exc:
                return ErrorResponse(error_code=ControlError.WORKFLOW_NOT_FOUND, message=str(exc))

        if isinstance(request, GetWorkflowExecutionRequest):
            try:
                return self.get_workflow_execution(request.workflow_id, request.execution_id, request.view)
            except WorkflowRecordUnavailableError as exc:
                return ErrorResponse(error_code=ControlError.WORKFLOW_NOT_FOUND, message=str(exc))
            except KeyError as exc:
                return ErrorResponse(error_code=ControlError.EXECUTION_NOT_FOUND, message=str(exc))

        if isinstance(request, SetTargetAliasRequest):
            try:
                return await self.set_alias(request.instance_id, request.alias)
            except KeyError as exc:
                return ErrorResponse(error_code=ControlError.UNKNOWN_CLIENT, message=str(exc))

        if isinstance(request, PingRequest):
            return PingResponse()

        return ErrorResponse(
            error_code=ControlError.UNKNOWN_REQUEST,
            message=f"unhandled request type: {type(request).__name__}",
        )

    def list_targets(self, instance_type: Optional[str] = None) -> ListTargetsResponse:
        clients = self._discovery.list_clients(instance_type)
        targets = [
            TargetInfo(
                instance_id=e.instance_id,
                instance_name=e.instance_name,
                alias=e.alias,
                instance_type=e.instance_type,
            )
            for e in clients.values()
        ]
        return ListTargetsResponse(targets=targets)

    async def set_alias(self, instance_id: str, alias: Optional[str]) -> SetTargetAliasResponse:
        return await self._set_alias_remote(instance_id, alias)

    async def _set_alias_remote(
        self,
        instance_id: str,
        alias: Optional[str],
        *,
        connect_timeout: float = DEFAULT_CONNECT_TIMEOUT,
    ) -> SetTargetAliasResponse:
        entry = self._discovery.get_client(instance_id)
        if entry is None:
            raise KeyError(f"unknown client: {instance_id}")

        result = await self._discovery.request(
            instance_id,
            InstanceSetAliasRequest(alias=alias),
            timeout=connect_timeout,
        )
        if not isinstance(result, InstanceSetAliasResult):
            raise WireModelError(f"unexpected response: {type(result).__name__}")
        self._discovery.set_alias(instance_id, result.alias)
        return SetTargetAliasResponse(success=True, instance_id=instance_id, alias=result.alias)

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
    ) -> InstanceExecResult:
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
        log.info(
            "Execute start: instance=%s workflow=%s execution=%s",
            instance_id,
            workflow_id,
            execution_id,
        )

        req = InstanceExecRequest(
            execution_id=execution_id,
            code=code,
            workflow_id=workflow_id,
            execution_name=name or None,
            request_id=request_id,
        )

        task = asyncio.create_task(self._receive_result(instance_id, req, workflow_id, execution_id))
        task.add_done_callback(ControlServer._consume_task_exception)

        try:
            return await asyncio.wait_for(asyncio.shield(task), timeout=early_return_window)
        except asyncio.TimeoutError:
            return InstanceExecResult(
                execution_id=execution_id,
                status=InstanceExecStatus.RUNNING,
                request_id=request_id,
            )
        except Exception:
            return InstanceExecResult(
                execution_id=execution_id,
                status=InstanceExecStatus.FAILED,
                error=InstanceExecError.CONNECTION_FAILED,
                request_id=request_id,
            )

    @staticmethod
    def _fail_execution(workflow_id: str, execution_id: str, error: InstanceExecError) -> None:
        WorkflowPersistence.finalize_execution_result(
            workflow_id,
            execution_id,
            InstanceExecStatus.FAILED,
            datetime.now(timezone.utc).isoformat(),
            error=error,
        )

    def handle_output_update(self, update: InstanceExecOutputUpdate) -> None:
        WorkflowPersistence.append_execution_output(
            update.workflow_id,
            update.execution_id,
            update.stdout_delta,
            update.stderr_delta,
        )

    @staticmethod
    def _consume_task_exception(task: asyncio.Task) -> None:
        """Retrieve exception (if any) so asyncio does not log it as unhandled."""
        if not task.cancelled():
            task.exception()

    async def _receive_result(
        self,
        instance_id: str,
        request: InstanceExecRequest,
        workflow_id: str,
        execution_id: str,
    ) -> InstanceExecResult:
        try:
            result = await self._discovery.request(
                instance_id,
                request,
                timeout=ControlServer.BACKGROUND_EXEC_TIMEOUT,
            )
            if not isinstance(result, InstanceExecResult):
                raise WireModelError(f"unexpected response: {type(result).__name__}")
            if result.error == InstanceExecError.BUSY:
                WorkflowPersistence.remove_execution(workflow_id, execution_id)
            else:
                WorkflowPersistence.finalize_execution_result(
                    workflow_id,
                    execution_id,
                    result.status,
                    datetime.now(timezone.utc).isoformat(),
                    result.traceback,
                    result.error,
                )
            log.info(
                "Execute finished: workflow=%s execution=%s status=%s error=%s",
                workflow_id,
                execution_id,
                result.status,
                result.error,
            )
            return result
        except asyncio.TimeoutError:
            log.warning(
                "Execution %s (workflow %s) timed out after %.0fs",
                execution_id,
                workflow_id,
                ControlServer.BACKGROUND_EXEC_TIMEOUT,
            )
            ControlServer._fail_execution(workflow_id, execution_id, InstanceExecError.EXECUTION_TIMEOUT)
            raise
        except WireModelError:
            ControlServer._fail_execution(workflow_id, execution_id, InstanceExecError.PROTOCOL_ERROR)
            raise
        except Exception:
            ControlServer._fail_execution(workflow_id, execution_id, InstanceExecError.CONNECTION_FAILED)
            raise
