import asyncio
import time
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

    async def list_clients(self) -> Dict[str, ClientEntry]:
        return await self._discovery.list_clients()

    async def set_alias(self, instance_id: str, alias: Optional[str]) -> None:
        await self._discovery.set_alias(instance_id, alias)

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
    ) -> ExecResult:
        # Validate workflow exists
        if not WorkflowPersistence.exists(workflow_id):
            raise FileNotFoundError(f"workflow not found: {workflow_id}")

        # Look up entry
        entry = await self._discovery.get_client(instance_id)
        if entry is None:
            raise KeyError(f"unknown client: {instance_id}")

        # Pre-write execution entry
        execution_id = WorkflowPersistence.append_running_execution(
            workflow_id, name, instance_id, code,
        )

        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(entry.exec_host, entry.exec_port),
            timeout=connect_timeout,
        )
        try:
            req = ExecRequest(
                execution_id=execution_id,
                code=code,
                workflow_id=workflow_id,
            )
            await AsyncJsonLineCodec.send(writer, req.to_dict())
            data = await AsyncJsonLineCodec.recv(reader)
            result = VersionedWireModel.parse_versioned(data)
            if not isinstance(result, ExecResult):
                raise RuntimeError(f"unexpected response: {type(result).__name__}")
            WorkflowPersistence.update_execution_result(
                workflow_id,
                execution_id,
                result.status,
                result.stdout,
                result.stderr,
                time.time(),
                result.traceback,
                result.error,
            )
            return result
        finally:
            writer.close()
