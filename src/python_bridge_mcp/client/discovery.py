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
    InstanceExecRegister,
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
    REGISTRATION_TIMEOUT = 10.0

    def __init__(
        self,
        name_hint: str,
        instance_name: str,
        runner: CodeRunner,
        instance_type: str = "",
        host: str = DEFAULT_HOST,
        port: int = REGISTRY_PORT,
        heartbeat_interval: float = HEARTBEAT_INTERVAL,
        pid: Optional[int] = None,
        registration_timeout: float = REGISTRATION_TIMEOUT,
        heartbeat_ack_timeout: Optional[float] = None,
    ):
        # Resent on every (re)registration so the server-assigned id never
        # accumulates suffixes on reconnect.
        self._name_hint = name_hint
        self._instance_id = name_hint
        self._instance_name = instance_name
        self._runner = runner
        self._instance_type = instance_type
        self._host = host
        self._port = port
        self._heartbeat_interval = heartbeat_interval
        self._registration_timeout = registration_timeout
        self._heartbeat_ack_timeout = (
            heartbeat_ack_timeout
            if heartbeat_ack_timeout is not None
            else max(heartbeat_interval * 2, 1.0)
        )
        self._pid = pid if pid is not None else os.getpid()

        self._stop_event = threading.Event()
        self._connected_event = threading.Event()
        self._state_lock = threading.Lock()
        self._state = DiscoveryState.STOPPED
        self._execution_lock: Optional[asyncio.Lock] = None
        self._loop: Optional[asyncio.AbstractEventLoop] = None
        self._cancel: Optional[asyncio.Event] = None
        # Two long connections: control (register/heartbeat) and exec (execution
        # traffic), so a long/blocked run can't stall heartbeats on one socket.
        self._ctrl_writer: Optional[asyncio.StreamWriter] = None
        self._ctrl_write_lock: Optional[asyncio.Lock] = None
        self._exec_writer: Optional[asyncio.StreamWriter] = None
        self._exec_write_lock: Optional[asyncio.Lock] = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    @property
    def instance_id(self) -> str:
        return self._instance_id

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
            loop = asyncio.new_event_loop()
            try:
                loop.run_until_complete(self._connect_and_serve())
                backoff = 0
            except Exception as exc:
                if self._stop_event.is_set():
                    break
                backoff = min(backoff * 2 + 1, self.MAX_BACKOFF)
                log.debug("Discovery disconnected (%s); retrying in %ds", exc, backoff)
                self._stop_event.wait(backoff)
            finally:
                # Clear refs BEFORE closing the loop — guarantees that whenever
                # self._loop is non-None the loop is still open, so stop() can
                # call_soon_threadsafe without racing against loop.close().
                self._loop = None
                self._cancel = None
                self._ctrl_writer = None
                self._ctrl_write_lock = None
                self._exec_writer = None
                self._exec_write_lock = None
                loop.close()

    def stop(self) -> None:
        """Close the owned runner and signal the client loop to stop."""
        self._stop_event.set()
        try:
            # Close the runner before cancelling async execution tasks. A
            # main-thread dispatcher must wake workers blocked on accepted work.
            self._runner.close()
        finally:
            loop, cancel = self._loop, self._cancel
            if loop is not None:
                try:
                    if cancel is not None:
                        loop.call_soon_threadsafe(cancel.set)
                    for writer in (self._ctrl_writer, self._exec_writer):
                        if writer is not None:
                            loop.call_soon_threadsafe(writer.close)
                except RuntimeError:
                    # Loop already closed by run()'s teardown — connection is
                    # down, nothing left to signal.
                    pass
            self._set_state(DiscoveryState.STOPPED)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    async def _connect_and_serve(self) -> None:
        self._loop = asyncio.get_running_loop()
        self._execution_lock = asyncio.Lock()
        cancel = asyncio.Event()
        self._cancel = cancel

        # control connection
        ctrl_reader, ctrl_writer = await asyncio.wait_for(
            asyncio.open_connection(
                self._host,
                self._port,
                limit=AsyncJsonLineCodec.READER_LIMIT,
            ),
            timeout=self._registration_timeout,
        )
        self._ctrl_writer = ctrl_writer
        self._ctrl_write_lock = asyncio.Lock()

        exec_reader = None
        exec_writer = None
        heartbeat_task = None
        ctrl_read_task = None
        exec_read_task = None
        exec_tasks = set()  # in-flight _handle_exec tasks, drained before close
        try:
            await self._send_control(
                InstanceRegister(
                    pid=self._pid,
                    name_hint=self._name_hint,
                    instance_name=self._instance_name,
                    instance_type=self._instance_type,
                )
            )
            ack = VersionedWireModel.parse_versioned(
                await AsyncJsonLineCodec.recv(
                    ctrl_reader,
                    timeout=self._registration_timeout,
                )
            )
            if not isinstance(ack, InstanceAck):
                raise RuntimeError("Registration rejected: unexpected response")
            if not ack.success:
                error = ack.message or ack.error_code or "unknown error"
                raise RuntimeError(f"Registration rejected: {error}")
            if ack.instance_id:
                self._instance_id = ack.instance_id

            # exec connection (opened once we know the assigned instance id)
            exec_reader, exec_writer = await asyncio.wait_for(
                asyncio.open_connection(
                    self._host,
                    self._port,
                    limit=AsyncJsonLineCodec.READER_LIMIT,
                ),
                timeout=self._registration_timeout,
            )
            self._exec_writer = exec_writer
            self._exec_write_lock = asyncio.Lock()
            await self._send_exec(
                InstanceExecRegister(instance_id=self._instance_id, pid=self._pid)
            )
            exec_ack = VersionedWireModel.parse_versioned(
                await AsyncJsonLineCodec.recv(
                    exec_reader,
                    timeout=self._registration_timeout,
                )
            )
            if not isinstance(exec_ack, InstanceAck) or not exec_ack.success:
                error = getattr(exec_ack, "message", "") or getattr(exec_ack, "error_code", "") or "unknown error"
                raise RuntimeError(f"Exec channel rejected: {error}")

            # Both channels up — advertise connectivity now, so a caller waiting
            # on wait_until_registered() can execute immediately.
            self._set_state(DiscoveryState.CONNECTED)
            log.info(
                "Discovery connected to %s:%d as %s",
                self._host,
                self._port,
                self._instance_id,
            )

            heartbeat_ack = asyncio.Event()
            heartbeat_task = asyncio.create_task(
                self._heartbeat_loop(cancel, heartbeat_ack)
            )
            ctrl_read_task = asyncio.create_task(
                self._control_read_loop(ctrl_reader, cancel, heartbeat_ack)
            )
            exec_read_task = asyncio.create_task(self._exec_read_loop(exec_reader, cancel, exec_tasks))

            done, _pending = await asyncio.wait(
                {heartbeat_task, ctrl_read_task, exec_read_task},
                return_when=asyncio.FIRST_COMPLETED,
            )
            # Re-raise the exited read loop's exception so run() applies backoff.
            for task in done:
                exc = task.exception()
                if exc is not None and not isinstance(exc, asyncio.CancelledError):
                    raise exc
        finally:
            cancel.set()
            loop_tasks = [t for t in (heartbeat_task, ctrl_read_task, exec_read_task) if t is not None]
            for task in loop_tasks:
                task.cancel()
            # Cancel in-flight executions and drain them BEFORE closing the
            # writers/loop, so their _OutputUpdateFlusher teardown doesn't fire
            # on a closed loop ("Task was destroyed but it is pending").
            for task in list(exec_tasks):
                task.cancel()
            if loop_tasks:
                await asyncio.gather(*loop_tasks, return_exceptions=True)
            if exec_tasks:
                await asyncio.gather(*exec_tasks, return_exceptions=True)
            exec_tasks.clear()
            for writer in (ctrl_writer, exec_writer):
                if writer is not None:
                    writer.close()
                    try:
                        await writer.wait_closed()
                    except Exception:
                        pass
            self._set_state(
                DiscoveryState.CONNECTING if not self._stop_event.is_set() else DiscoveryState.STOPPED
            )

    async def _heartbeat_loop(
        self,
        cancel: asyncio.Event,
        heartbeat_ack: asyncio.Event,
    ) -> None:
        while True:
            try:
                await asyncio.wait_for(cancel.wait(), timeout=self._heartbeat_interval)
                return  # cancel was set
            except asyncio.TimeoutError:
                pass
            heartbeat_ack.clear()
            await self._send_control(InstanceHeartbeat(instance_id=self._instance_id))
            try:
                await asyncio.wait_for(
                    heartbeat_ack.wait(),
                    timeout=self._heartbeat_ack_timeout,
                )
            except asyncio.TimeoutError:
                raise ConnectionError(
                    "heartbeat acknowledgement timed out after %.1fs"
                    % self._heartbeat_ack_timeout
                )

    async def _control_read_loop(
        self,
        reader: asyncio.StreamReader,
        cancel: asyncio.Event,
        heartbeat_ack: asyncio.Event,
    ) -> None:
        while not cancel.is_set():
            msg = VersionedWireModel.parse_versioned(await AsyncJsonLineCodec.recv(reader))
            if isinstance(msg, InstanceAck):
                if not msg.success:
                    raise RuntimeError(msg.message or msg.error_code or "backend rejected request")
                heartbeat_ack.set()
            else:
                raise WireModelError(
                    f"unexpected message type on control channel: {type(msg).__name__}"
                )

    async def _exec_read_loop(
        self,
        reader: asyncio.StreamReader,
        cancel: asyncio.Event,
        exec_tasks: set,
    ) -> None:
        while not cancel.is_set():
            msg = VersionedWireModel.parse_versioned(await AsyncJsonLineCodec.recv(reader))
            if isinstance(msg, InstanceExecRequest):
                task = asyncio.create_task(self._handle_exec(msg))
                exec_tasks.add(task)
                task.add_done_callback(exec_tasks.discard)
            else:
                raise WireModelError(
                    f"unexpected message type on exec channel: {type(msg).__name__}"
                )

    async def _handle_exec(self, msg: InstanceExecRequest) -> None:
        assert self._execution_lock is not None
        if self._execution_lock.locked():
            await self._send_exec(
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
                self._send_exec,
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
                    filename=msg.filename,
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
        await self._send_exec(result)

    async def _send_control(self, msg: VersionedWireModel) -> None:
        if self._ctrl_writer is None or self._ctrl_write_lock is None:
            raise ConnectionError("not connected")
        async with self._ctrl_write_lock:
            await AsyncJsonLineCodec.send(self._ctrl_writer, msg.to_dict())

    async def _send_exec(self, msg: VersionedWireModel) -> None:
        if self._exec_writer is None or self._exec_write_lock is None:
            raise ConnectionError("not connected")
        async with self._exec_write_lock:
            await AsyncJsonLineCodec.send(self._exec_writer, msg.to_dict())

    def _set_state(self, state: DiscoveryState) -> None:
        with self._state_lock:
            self._state = state
        if state == DiscoveryState.CONNECTED:
            self._connected_event.set()
        else:
            self._connected_event.clear()
