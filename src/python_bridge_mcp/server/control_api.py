import asyncio
from typing import Optional

from .control_models import (
    ControlExecuteRequest,
    ErrorResponse,
    GetWorkflowExecutionRequest,
    GetWorkflowOverviewRequest,
    ListTargetsRequest,
    SetTargetAliasRequest,
    StartWorkflowRequest,
    StartWorkflowResponse,
)
from .control_server import ControlServer
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError
from ..shared.workflow_persistence import WorkflowRecordUnavailableError


class ControlApi:
    DEFAULT_HOST = "localhost"
    DEFAULT_PORT = 6322

    def __init__(
        self,
        control: ControlServer,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
    ) -> None:
        self._control = control
        self._host = host
        self._port = port
        self._server: Optional[asyncio.AbstractServer] = None

    async def run(self) -> None:
        self._server = await asyncio.start_server(
            self._handle_connection,
            self._host,
            self._port,
            limit=AsyncJsonLineCodec.READER_LIMIT,
        )
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
        except WireModelError as exc:
            response = ErrorResponse(error_code="protocol_error", message=str(exc))
        except Exception as exc:
            response = ErrorResponse(error_code="internal_error", message=str(exc))
        finally:
            if response is not None:
                await AsyncJsonLineCodec.send(writer, response.to_dict())
            writer.close()

    async def _dispatch(self, request: VersionedWireModel) -> VersionedWireModel:
        if isinstance(request, ListTargetsRequest):
            return self._control.list_targets(request.instance_type)

        if isinstance(request, StartWorkflowRequest):
            wf_id = self._control.start_workflow(request.name, request.description)
            return StartWorkflowResponse(workflow_id=wf_id)

        if isinstance(request, ControlExecuteRequest):
            try:
                return await self._control.execute(
                    request.instance_id, request.code, request.workflow_id, request.name
                )
            except KeyError as exc:
                return ErrorResponse(error_code="unknown_client", message=str(exc))

        if isinstance(request, GetWorkflowOverviewRequest):
            try:
                return self._control.get_workflow_overview(request.workflow_id)
            except WorkflowRecordUnavailableError as exc:
                return ErrorResponse(error_code="workflow_not_found", message=str(exc))

        if isinstance(request, GetWorkflowExecutionRequest):
            try:
                return self._control.get_workflow_execution(
                    request.workflow_id, request.execution_id, request.view
                )
            except WorkflowRecordUnavailableError as exc:
                return ErrorResponse(error_code="workflow_not_found", message=str(exc))
            except KeyError as exc:
                return ErrorResponse(error_code="execution_not_found", message=str(exc))

        if isinstance(request, SetTargetAliasRequest):
            try:
                return self._control.set_alias(request.instance_id, request.alias)
            except KeyError as exc:
                return ErrorResponse(error_code="unknown_client", message=str(exc))

        return ErrorResponse(
            error_code="unknown_request",
            message=f"unhandled request type: {type(request).__name__}",
        )
