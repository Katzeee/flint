import asyncio
from typing import Optional

from .control_models import (
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
)
from ..shared.exec_models import ExecResult
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel
from ..shared.workflow_persistence import WorkflowRecordUnavailableError


class BackendError(Exception):
    def __init__(self, error_code: str, message: str) -> None:
        super().__init__(message)
        self.error_code = error_code


class BackendClient:
    DEFAULT_HOST = "localhost"
    DEFAULT_PORT = 6322

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
    ) -> None:
        self._host = host
        self._port = port

    ROUNDTRIP_TIMEOUT: float = 30.0

    async def _roundtrip(
        self,
        request: VersionedWireModel,
        timeout: Optional[float] = ROUNDTRIP_TIMEOUT,
    ) -> VersionedWireModel:
        reader, writer = await asyncio.open_connection(
            self._host, self._port, limit=AsyncJsonLineCodec.READER_LIMIT
        )
        try:
            await AsyncJsonLineCodec.send(writer, request.to_dict())
            data = await AsyncJsonLineCodec.recv(reader, timeout=timeout)
            return VersionedWireModel.parse_versioned(data)
        finally:
            writer.close()
            await writer.wait_closed()

    def _raise_if_error(self, response: VersionedWireModel) -> None:
        if not isinstance(response, ErrorResponse):
            return
        if response.error_code == "workflow_not_found":
            raise WorkflowRecordUnavailableError(response.message)
        if response.error_code in ("unknown_client", "execution_not_found"):
            raise KeyError(response.message)
        raise BackendError(response.error_code, response.message)

    async def ping(self) -> bool:
        resp = await self._roundtrip(PingRequest())
        return (
            isinstance(resp, PingResponse)
            and resp.ok is True
            and resp.service == "python-bridge-backend"
            and resp.ready is True
        )

    async def list_targets(self, dcc_type: Optional[str] = None) -> ListTargetsResponse:
        resp = await self._roundtrip(ListTargetsRequest(instance_type=dcc_type))
        self._raise_if_error(resp)
        assert isinstance(resp, ListTargetsResponse)
        return resp

    async def start_workflow(self, name: str, description: str = "") -> str:
        resp = await self._roundtrip(StartWorkflowRequest(name=name, description=description))
        self._raise_if_error(resp)
        assert isinstance(resp, StartWorkflowResponse)
        return resp.workflow_id

    async def execute(
        self,
        instance_id: str,
        code: str,
        workflow_id: str,
        name: str = "",
    ) -> ExecResult:
        resp = await self._roundtrip(
            ControlExecuteRequest(
                instance_id=instance_id,
                code=code,
                workflow_id=workflow_id,
                name=name,
            )
        )
        self._raise_if_error(resp)
        assert isinstance(resp, ExecResult)
        return resp

    async def get_workflow_overview(self, workflow_id: str) -> GetWorkflowOverviewResponse:
        resp = await self._roundtrip(GetWorkflowOverviewRequest(workflow_id=workflow_id))
        self._raise_if_error(resp)
        assert isinstance(resp, GetWorkflowOverviewResponse)
        return resp

    async def get_workflow_execution(
        self,
        workflow_id: str,
        execution_id: str,
        view: str = "summary",
    ) -> GetWorkflowExecutionResponse:
        resp = await self._roundtrip(
            GetWorkflowExecutionRequest(
                workflow_id=workflow_id,
                execution_id=execution_id,
                view=view,
            )
        )
        self._raise_if_error(resp)
        assert isinstance(resp, GetWorkflowExecutionResponse)
        return resp

    async def set_alias(
        self, instance_id: str, alias: Optional[str] = None
    ) -> SetTargetAliasResponse:
        resp = await self._roundtrip(
            SetTargetAliasRequest(instance_id=instance_id, alias=alias)
        )
        self._raise_if_error(resp)
        assert isinstance(resp, SetTargetAliasResponse)
        return resp
