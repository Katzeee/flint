from __future__ import annotations

import asyncio
import logging
import os
import threading
from enum import Enum
from typing import Awaitable, Callable, Optional, Tuple, Union

from ..shared.constants import DEFAULT_HOST, REGISTRY_PORT
from ..shared.instance_control_models import (
    InstanceExecError,
    InstanceExecOutputUpdate,
    InstanceExecRequest,
    InstanceExecResult,
    InstanceExecStatus,
    InstanceAck,
    InstanceHeartbeat,
    InstanceRegister,
)
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError
from ..shared.text_buffer import ThreadSafeTextBuffer
from .code_runner import CodeRunner

log = logging.getLogger(__name__)


class DiscoveryState(Enum):
    CONNECTING = "connecting"
    CONNECTED = "connected"
    STOPPED = "stopped"


class _OutputUpdateFlusher:
    def __init__(
        self,
        request: InstanceExecRequest,
        out: ThreadSafeTextBuffer,
        err: ThreadSafeTextBuffer,
        send: Callable[[VersionedWireModel], Awaitable[None]],
        interval: float,
        max_chars: int,
    ) -> None:
        self._request = request
        self._out = out
        self._err = err
        self._send = send
        self._interval = interval
        self._max_chars = max_chars
        self._stdout_pos = 0
        self._stderr_pos = 0
        self._sequence = 0
        self._stop_event = asyncio.Event()
        self._task: Optional[asyncio.Task] = None

    def start(self) -> None:
        self._task = asyncio.create_task(self._run())

    async def stop(self) -> None:
        self._stop_event.set()
        if self._task is not None:
            await asyncio.gather(self._task, return_exceptions=True)
        await self.flush()

    async def flush(self) -> None:
        stdout_delta, stderr_delta = self._pending_output()
        while stdout_delta or stderr_delta:
            stdout_chunk = stdout_delta[: self._max_chars]
            remaining = self._max_chars - len(stdout_chunk)
            stderr_chunk = stderr_delta[:remaining]
            await self._send_update(stdout_chunk, stderr_chunk)
            stdout_delta = stdout_delta[len(stdout_chunk) :]
            stderr_delta = stderr_delta[len(stderr_chunk) :]

    async def _run(self) -> None:
        while not self._stop_event.is_set():
            try:
                await asyncio.wait_for(self._stop_event.wait(), timeout=self._interval)
            except asyncio.TimeoutError:
                await self.flush()

    def _pending_output(self) -> Tuple[str, str]:
        stdout_value = self._out.getvalue()
        stderr_value = self._err.getvalue()
        return (
            stdout_value[self._stdout_pos :],
            stderr_value[self._stderr_pos :],
        )

    async def _send_update(self, stdout_delta: str, stderr_delta: str) -> None:
        self._sequence += 1
        await self._send(
            InstanceExecOutputUpdate(
                execution_id=self._request.execution_id,
                workflow_id=self._request.workflow_id,
                sequence=self._sequence,
                stdout_delta=stdout_delta,
                stderr_delta=stderr_delta,
                request_id=self._request.request_id,
            )
        )
        self._stdout_pos += len(stdout_delta)
        self._stderr_pos += len(stderr_delta)


class DiscoveryClient:
    HEARTBEAT_INTERVAL = 5
    MAX_BACKOFF = 30
    OUTPUT_FLUSH_INTERVAL = 2.0
    OUTPUT_UPDATE_MAX_CHARS = 256 * 1024

    def __init__(
        self,
        instance_id: str,
        instance_name: str,
        runner: CodeRunner,
        alias: Optional[str] = None,
        alias_getter: Optional[Callable[[], Optional[str]]] = None,
        instance_type: str = "",
        host: str = DEFAULT_HOST,
        port: int = REGISTRY_PORT,
        heartbeat_interval: float = HEARTBEAT_INTERVAL,
        pid: Optional[int] = None,
    ):
        self._instance_id = instance_id
        self._instance_name = instance_name
        self._runner = runner
        self._alias = alias
        self._alias_getter = alias_getter
        self._instance_type = instance_type
        self._host = host
        self._port = port
        self._heartbeat_interval = heartbeat_interval
        self._pid = pid if pid is not None else os.getpid()

        self._stop_event = threading.Event()
        self._connected_event = threading.Event()
        self._state_lock = threading.Lock()
        self._state = DiscoveryState.STOPPED
        self._alias_lock = threading.Lock()
        self._execution_lock: Optional[asyncio.Lock] = None
        self._writer: Optional[asyncio.StreamWriter] = None
        self._write_lock: Optional[asyncio.Lock] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    @property
    def state(self) -> DiscoveryState:
        with self._state_lock:
            return self._state

    @property
    def host(self) -> str:
        return self._host

    @property
    def port(self) -> int:
        return self._port

    def wait_until_registered(self, timeout: Union[int, float] = 10) -> bool:
        return self._connected_event.wait(timeout)

    def is_online(self) -> bool:
        return self._connected_event.is_set()

    def get_connection_state(self) -> DiscoveryState:
        return self.state

    def run(self) -> None:
        """Run the client loop forever; returns only after stop() is called."""
        backoff = 0
        while not self._stop_event.is_set():
            self._set_state(DiscoveryState.CONNECTING)
            try:
                asyncio.run(self._connect_and_serve())
                backoff = 0
            except Exception as exc:
                if self._stop_event.is_set():
                    break
                backoff = min(backoff * 2 + 1, self.MAX_BACKOFF)
                log.debug("Discovery disconnected (%s); retrying in %ds", exc, backoff)
                self._stop_event.wait(backoff)

    def stop(self) -> None:
        """Signal the client to stop and return from run()."""
        self._stop_event.set()
        if self._loop is not None and self._writer is not None:
            self._loop.call_soon_threadsafe(self._writer.close)
        self._set_state(DiscoveryState.STOPPED)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _current_alias(self) -> Optional[str]:
        if self._alias_getter is not None:
            alias = self._alias_getter()
        else:
            with self._alias_lock:
                alias = self._alias
        alias = alias.strip() if alias is not None else None
        return alias or None

    async def _connect_and_serve(self) -> None:
        self._loop = asyncio.get_running_loop()
        self._execution_lock = asyncio.Lock()
        reader, writer = await asyncio.open_connection(
            self._host,
            self._port,
            limit=AsyncJsonLineCodec.READER_LIMIT,
        )
        self._writer = writer
        self._write_lock = asyncio.Lock()
        try:
            await self._send(
                InstanceRegister(
                    pid=self._pid,
                    instance_id=self._instance_id,
                    instance_name=self._instance_name,
                    alias=self._current_alias(),
                    instance_type=self._instance_type,
                )
            )
            ack = VersionedWireModel.parse_versioned(await AsyncJsonLineCodec.recv(reader))
            if not isinstance(ack, InstanceAck):
                raise RuntimeError("Registration rejected: unexpected response")
            if not ack.success:
                error = ack.message or ack.error_code or "unknown error"
                raise RuntimeError(f"Registration rejected: {error}")
            self._set_state(DiscoveryState.CONNECTED)
            log.info(
                "Discovery connected to %s:%d as %s",
                self._host,
                self._port,
                self._instance_id,
            )

            heartbeat_task = asyncio.create_task(self._heartbeat_loop())
            try:
                await self._read_loop(reader)
            finally:
                heartbeat_task.cancel()
                await asyncio.gather(heartbeat_task, return_exceptions=True)
        finally:
            writer.close()
            await writer.wait_closed()
            self._writer = None
            self._write_lock = None
            self._loop = None
            self._set_state(DiscoveryState.CONNECTING if not self._stop_event.is_set() else DiscoveryState.STOPPED)

    async def _heartbeat_loop(self) -> None:
        while not self._stop_event.is_set():
            await asyncio.sleep(self._heartbeat_interval)
            if self._stop_event.is_set():
                return
            await self._send(InstanceHeartbeat(instance_id=self._instance_id))

    async def _read_loop(self, reader: asyncio.StreamReader) -> None:
        while not self._stop_event.is_set():
            data = await AsyncJsonLineCodec.recv(reader)
            msg = VersionedWireModel.parse_versioned(data)
            if isinstance(msg, InstanceAck):
                if not msg.success:
                    raise RuntimeError(msg.message or msg.error_code or "backend rejected request")
            elif isinstance(msg, InstanceExecRequest):
                asyncio.create_task(self._handle_exec(msg))
            else:
                raise WireModelError(f"unexpected message type: {type(msg).__name__}")

    async def _handle_exec(self, msg: InstanceExecRequest) -> None:
        assert self._execution_lock is not None
        if self._execution_lock.locked():
            await self._send(
                InstanceExecResult(
                    execution_id=msg.execution_id,
                    status=InstanceExecStatus.FAILED,
                    error=InstanceExecError.BUSY,
                    request_id=msg.request_id,
                )
            )
            return

        async with self._execution_lock:
            out = ThreadSafeTextBuffer()
            err = ThreadSafeTextBuffer()
            flusher = _OutputUpdateFlusher(
                msg,
                out,
                err,
                self._send,
                self.OUTPUT_FLUSH_INTERVAL,
                self.OUTPUT_UPDATE_MAX_CHARS,
            )
            flusher.start()
            try:
                result = await self._runner.async_execute(
                    msg.execution_id,
                    msg.code,
                    out,
                    err,
                )
                result.request_id = msg.request_id
            except Exception as exc:
                result = InstanceExecResult(
                    execution_id=msg.execution_id,
                    status=InstanceExecStatus.FAILED,
                    error=str(exc),
                    request_id=msg.request_id,
                )
            finally:
                await flusher.stop()
        await self._send(result)

    async def _send(self, msg: VersionedWireModel) -> None:
        if self._writer is None or self._write_lock is None:
            raise ConnectionError("not connected")
        async with self._write_lock:
            await AsyncJsonLineCodec.send(self._writer, msg.to_dict())

    def _set_state(self, state: DiscoveryState) -> None:
        with self._state_lock:
            self._state = state
        if state == DiscoveryState.CONNECTED:
            self._connected_event.set()
        else:
            self._connected_event.clear()
