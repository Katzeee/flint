import asyncio
from typing import Optional

from .control_models import (
    ControlError,
    ControlExecuteRequest,
    ErrorResponse,
    GetWorkflowExecutionRequest,
    GetWorkflowExecutionResponse,
    ListInstancesRequest,
    ListInstancesResponse,
    PingRequest,
    PingResponse,
    StartWorkflowRequest,
    StartWorkflowResponse,
)
from ..shared.constants import DEFAULT_HOST, CONTROL_API_PORT
from ..shared.instance_control_models import InstanceExecResult
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel
from ..shared.workflow_persistence import WorkflowRecordUnavailableError


class BackendError(Exception):
    def __init__(self, error_code: ControlError, message: str) -> None:
        super().__init__(message)
        self.error_code = error_code


class BackendClient:
    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = CONTROL_API_PORT,
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
        if response.error_code == ControlError.WORKFLOW_NOT_FOUND:
            raise WorkflowRecordUnavailableError(response.message)
        if response.error_code in (ControlError.UNKNOWN_CLIENT, ControlError.EXECUTION_NOT_FOUND):
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

    async def list_instances(self, instance_type: Optional[str] = None) -> ListInstancesResponse:
        resp = await self._roundtrip(ListInstancesRequest(instance_type=instance_type))
        self._raise_if_error(resp)
        assert isinstance(resp, ListInstancesResponse)
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
    ) -> InstanceExecResult:
        resp = await self._roundtrip(
            ControlExecuteRequest(
                instance_id=instance_id,
                code=code,
                workflow_id=workflow_id,
                name=name,
            )
        )
        self._raise_if_error(resp)
        assert isinstance(resp, InstanceExecResult)
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

