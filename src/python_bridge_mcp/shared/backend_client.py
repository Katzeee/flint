"""Minimal sync client for the backend control API.

Uses only shared/ components so it can be distributed with the client package.
Speaks the control wire protocol via raw JSON dicts.
"""
import socket
import time
from typing import Any, Dict, List, Optional

from .constants import CONTROL_API_PORT, DEFAULT_HOST
from .jsonline import SyncJsonLineCodec

_PROTOCOL_VERSION = 1


class BackendClientError(Exception):
    def __init__(self, error_code: str, message: str) -> None:
        super().__init__(message)
        self.error_code = error_code


class BackendControlClient:
    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = CONTROL_API_PORT,
        timeout: float = 30.0,
    ) -> None:
        self._host = host
        self._port = port
        self._timeout = timeout

    def _roundtrip(self, request: Dict[str, Any]) -> Dict[str, Any]:
        conn = socket.create_connection((self._host, self._port), timeout=self._timeout)
        try:
            SyncJsonLineCodec.send(conn, request)
            resp = SyncJsonLineCodec.recv(conn)
        finally:
            conn.close()
        if resp.get("type") == "ErrorResponse":
            raise BackendClientError(resp.get("error_code", "unknown"), resp.get("message", ""))
        return resp

    def _req(self, type_name: str, **fields: Any) -> Dict[str, Any]:
        return {"type": type_name, "version": _PROTOCOL_VERSION, **fields}

    def list_instances(self, instance_type: Optional[str] = None) -> List[Dict[str, Any]]:
        req = self._req("ListInstancesRequest")
        if instance_type is not None:
            req["instance_type"] = instance_type
        return self._roundtrip(req).get("instances", [])

    def start_workflow(self, name: str, description: str = "") -> str:
        resp = self._roundtrip(self._req("StartWorkflowRequest", name=name, description=description))
        return resp["workflow_id"]

    def execute(
        self,
        instance_id: str,
        code: str,
        workflow_id: str,
        name: str = "",
        filename: Optional[str] = None,
    ) -> Dict[str, Any]:
        req = self._req(
            "ControlExecuteRequest",
            instance_id=instance_id,
            code=code,
            workflow_id=workflow_id,
            name=name,
        )
        if filename is not None:
            req["filename"] = filename
        return self._roundtrip(req)

    def get_execution(
        self,
        workflow_id: str,
        execution_id: str,
        view: str = "summary",
    ) -> Dict[str, Any]:
        return self._roundtrip(
            self._req(
                "GetWorkflowExecutionRequest",
                workflow_id=workflow_id,
                execution_id=execution_id,
                view=view,
            )
        )

    def wait_for_completion(
        self,
        workflow_id: str,
        execution_id: str,
        poll_interval: float = 0.5,
        timeout: float = 600.0,
    ) -> Dict[str, Any]:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            resp = self.get_execution(workflow_id, execution_id)
            if resp.get("status") not in ("pending", "running"):
                return resp
            time.sleep(poll_interval)
        raise TimeoutError(
            f"Execution {execution_id} did not complete within {timeout:.0f}s"
        )
