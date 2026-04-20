import asyncio
import logging
import threading
from typing import Optional

from ..shared.exec_models import (
    ExecError,
    ExecRequest,
    ExecResult,
    ExecStatus,
    SetAliasRequest,
    SetAliasResult,
)
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError
from ..shared.text_buffer import ThreadSafeTextBuffer
from .code_runner import CodeRunner
from .periodic_flusher import PeriodicFlusher

log = logging.getLogger(__name__)


class ExecListener:
    """Async TCP server that accepts ExecRequest and returns ExecResult."""

    DEFAULT_CONN_TIMEOUT = 30.0
    FLUSH_INTERVAL = 0.5

    def __init__(
        self,
        host: str,
        port: int,
        runner: CodeRunner,
        conn_timeout: float = DEFAULT_CONN_TIMEOUT,
    ) -> None:
        self._host = host
        self._port = port
        self._runner = runner
        self._conn_timeout = conn_timeout
        self._server: Optional[asyncio.AbstractServer] = None
        self._execution_lock = asyncio.Lock()
        self._alias: Optional[str] = None
        self._alias_lock = threading.Lock()

    def get_alias(self) -> Optional[str]:
        with self._alias_lock:
            return self._alias

    def set_alias(self, alias: Optional[str]) -> Optional[str]:
        normalized = alias.strip() if alias is not None else None
        normalized = normalized or None
        with self._alias_lock:
            self._alias = normalized
            return self._alias

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
        result: Optional[VersionedWireModel] = None
        try:
            data = await asyncio.wait_for(AsyncJsonLineCodec.recv(reader), timeout=self._conn_timeout)
            msg = VersionedWireModel.parse_versioned(data)
            if isinstance(msg, SetAliasRequest):
                result = SetAliasResult(success=True, alias=self.set_alias(msg.alias))
            elif not isinstance(msg, ExecRequest):
                result = ExecResult(
                    execution_id="",
                    status=ExecStatus.FAILED,
                    stdout="",
                    stderr="",
                    error=ExecError.PROTOCOL_ERROR,
                )
            elif self._execution_lock.locked():
                log.warning("Rejecting execution %s: BUSY", msg.execution_id)
                result = ExecResult(
                    execution_id=msg.execution_id,
                    status=ExecStatus.FAILED,
                    stdout="",
                    stderr="",
                    error=ExecError.BUSY,
                )
            else:
                log.info(
                    "Execution %s start (workflow %s)",
                    msg.execution_id, msg.workflow_id,
                )
                async with self._execution_lock:
                    out = ThreadSafeTextBuffer()
                    err = ThreadSafeTextBuffer()
                    flusher = PeriodicFlusher(
                        self.FLUSH_INTERVAL,
                        msg.workflow_id,
                        msg.execution_id,
                        out,
                        err,
                    )
                    flusher.start()
                    try:
                        result = await self._runner.async_execute(
                            msg.execution_id,
                            msg.code,
                            out,
                            err,
                        )
                    finally:
                        flusher.stop()
                log.info(
                    "Execution %s finished status=%s",
                    msg.execution_id, result.status if result else None,
                )
                if result is not None and msg.request_id:
                    result.request_id = msg.request_id
        except (asyncio.TimeoutError, ConnectionError, WireModelError, ValueError) as exc:
            log.warning("ExecListener connection error: %s", exc)
            result = ExecResult(
                execution_id="",
                status=ExecStatus.FAILED,
                stdout="",
                stderr="",
                error=str(exc),
            )
        finally:
            if result is not None:
                await AsyncJsonLineCodec.send(writer, result.to_dict())
            writer.close()
