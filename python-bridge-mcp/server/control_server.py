import asyncio
from datetime import datetime, timezone
from typing import Dict, Optional

from .registry import ClientEntry, Registry
from ..shared.exec_models import ExecError, ExecRequest, ExecResult, ExecStatus
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel
from ..shared.workflow_persistence import WorkflowPersistence


class ControlServer:
    DEFAULT_CONNECT_TIMEOUT: float = 10.0
    DEFAULT_EARLY_RETURN_WINDOW: float = 5.0

    def __init__(self, discovery: Registry) -> None:
        self._discovery = discovery

    def list_clients(self, instance_type: Optional[str] = None) -> Dict[str, ClientEntry]:
        return self._discovery.list_clients(instance_type)

    def set_alias(self, instance_id: str, alias: Optional[str]) -> None:
        self._discovery.set_alias(instance_id, alias)

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

        execution_id = WorkflowPersistence.append_running_execution(
            workflow_id,
            name,
            instance_id,
            code,
        )

        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(entry.exec_host, entry.exec_port),
            timeout=connect_timeout,
        )

        req = ExecRequest(
            execution_id=execution_id,
            code=code,
            workflow_id=workflow_id,
        )
        await AsyncJsonLineCodec.send(writer, req.to_dict())

        task = asyncio.create_task(self._receive_result(reader, writer, workflow_id, execution_id))

        try:
            return await asyncio.wait_for(asyncio.shield(task), timeout=early_return_window)
        except asyncio.TimeoutError:
            return ExecResult(
                execution_id=execution_id,
                status=ExecStatus.RUNNING,
                stdout="",
                stderr="",
            )

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
                raise RuntimeError(f"unexpected response: {type(result).__name__}")
            if result.error == ExecError.BUSY:
                WorkflowPersistence.remove_execution(workflow_id, execution_id)
            else:
                WorkflowPersistence.update_execution_result(
                    workflow_id,
                    execution_id,
                    result.status,
                    result.stdout,
                    result.stderr,
                    datetime.now(timezone.utc).isoformat(),
                    result.traceback,
                    result.error,
                )
            return result
        finally:
            writer.close()
