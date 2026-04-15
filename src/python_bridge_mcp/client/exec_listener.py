import asyncio
from typing import Optional

from ..shared.exec_models import ExecError, ExecRequest, ExecResult, ExecStatus
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError
from ..shared.text_buffer import ThreadSafeTextBuffer
from .code_runner import CodeRunner
from .periodic_flusher import PeriodicFlusher


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
        result: Optional[ExecResult] = None
        try:
            data = await asyncio.wait_for(AsyncJsonLineCodec.recv(reader), timeout=self._conn_timeout)
            msg = VersionedWireModel.parse_versioned(data)
            if not isinstance(msg, ExecRequest):
                result = ExecResult(
                    execution_id="",
                    status=ExecStatus.FAILED,
                    stdout="",
                    stderr="",
                    error=f"unexpected message: {type(msg).__name__}",
                )
            elif self._execution_lock.locked():
                result = ExecResult(
                    execution_id=msg.execution_id,
                    status=ExecStatus.FAILED,
                    stdout="",
                    stderr="",
                    error=ExecError.BUSY,
                )
            else:
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
                if result is not None and msg.request_id:
                    result.request_id = msg.request_id
        except (asyncio.TimeoutError, ConnectionError, WireModelError, ValueError) as exc:
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
