import asyncio
import time
import uuid
from typing import Dict, Optional

from .registry import ClientEntry, Registry
from ..shared.exec_models import ExecRequest, ExecResult
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel
from ..shared.workflow_persistence import WorkflowPersistence


class ControlServer:

    DEFAULT_CONNECT_TIMEOUT: float = 10.0

    def __init__(self, discovery: Registry) -> None:
        self._discovery = discovery
        self._wf_counter = 0

    async def list_clients(self) -> Dict[str, ClientEntry]:
        return await self._discovery.list_clients()

    async def set_alias(self, instance_id: str, alias: Optional[str]) -> None:
        await self._discovery.set_alias(instance_id, alias)

    def start_workflow(self, name: str, workflow_file_path: str) -> str:
        ts = time.strftime("%Y%m%d_%H%M%S")
        self._wf_counter += 1
        workflow_id = f"{name}_{ts}_{self._wf_counter}"
        WorkflowPersistence.create_workflow(
            workflow_file_path, workflow_id, name,
        )
        return workflow_id

    async def execute(
        self,
        instance_id: str,
        code: str,
        workflow_id: str,
        workflow_file_path: str,
        *,
        connect_timeout: float = DEFAULT_CONNECT_TIMEOUT,
    ) -> ExecResult:
        if not WorkflowPersistence.exists(workflow_file_path):
            raise FileNotFoundError(
                f"workflow file not found: {workflow_file_path}"
            )

        entry = await self._discovery.get_client(instance_id)
        if entry is None:
            raise KeyError(f"unknown client: {instance_id}")

        request_id = str(uuid.uuid4())
        WorkflowPersistence.append_running_execution(
            workflow_file_path, request_id, workflow_id, instance_id, code,
        )

        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(entry.exec_host, entry.exec_port),
            timeout=connect_timeout,
        )
        try:
            req = ExecRequest(
                request_id=request_id, code=code,
                workflow_id=workflow_id, workflow_file_path=workflow_file_path,
            )
            await AsyncJsonLineCodec.send(writer, req.to_dict())
            data = await AsyncJsonLineCodec.recv(reader)
            result = VersionedWireModel.parse_versioned(data)
            if not isinstance(result, ExecResult):
                raise RuntimeError(
                    f"unexpected response: {type(result).__name__}"
                )
            WorkflowPersistence.update_execution_result(
                workflow_file_path, request_id,
                result.status, result.stdout, result.stderr,
                time.time(), result.traceback, result.error,
            )
            return result
        finally:
            writer.close()
