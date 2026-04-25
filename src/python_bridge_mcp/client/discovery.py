from __future__ import annotations

import asyncio
import logging
import os
import threading
from enum import Enum
from typing import Callable, Optional, Union

from ..shared.discovery_models import AckDiscovery, HeartbeatDiscovery, RegisterDiscovery
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


class DiscoveryState(Enum):
    CONNECTING = "connecting"
    CONNECTED = "connected"
    STOPPED = "stopped"


class DiscoveryClient:
    DEFAULT_HOST = "localhost"
    DEFAULT_PORT = 6321
    HEARTBEAT_INTERVAL = 5
    MAX_BACKOFF = 30
    FLUSH_INTERVAL = 0.5

    def __init__(
        self,
        instance_id: str,
        instance_name: str,
        runner: CodeRunner,
        alias: Optional[str] = None,
        alias_getter: Optional[Callable[[], Optional[str]]] = None,
        instance_type: str = "",
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
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

    def _set_alias(self, alias: Optional[str]) -> Optional[str]:
        normalized = alias.strip() if alias is not None else None
        normalized = normalized or None
        with self._alias_lock:
            self._alias = normalized
            return self._alias

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
            await self._send(RegisterDiscovery(
                pid=self._pid,
                instance_id=self._instance_id,
                instance_name=self._instance_name,
                alias=self._current_alias(),
                instance_type=self._instance_type,
            ))
            ack = VersionedWireModel.parse_versioned(await AsyncJsonLineCodec.recv(reader))
            if not isinstance(ack, AckDiscovery):
                raise RuntimeError("Registration rejected: unexpected response")
            if not ack.success:
                raise RuntimeError(f"Registration rejected: {ack.error}")
            self._set_state(DiscoveryState.CONNECTED)
            log.info(
                "Discovery connected to %s:%d as %s",
                self._host, self._port, self._instance_id,
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
            await self._send(HeartbeatDiscovery(instance_id=self._instance_id))

    async def _read_loop(self, reader: asyncio.StreamReader) -> None:
        while not self._stop_event.is_set():
            data = await AsyncJsonLineCodec.recv(reader)
            msg = VersionedWireModel.parse_versioned(data)
            if isinstance(msg, AckDiscovery):
                if not msg.success:
                    raise RuntimeError(msg.error or "backend rejected request")
            elif isinstance(msg, ExecRequest):
                asyncio.create_task(self._handle_exec(msg))
            elif isinstance(msg, SetAliasRequest):
                asyncio.create_task(self._handle_set_alias(msg))
            else:
                raise WireModelError(f"unexpected message type: {type(msg).__name__}")

    async def _handle_set_alias(self, msg: SetAliasRequest) -> None:
        await self._send(SetAliasResult(
            success=True,
            alias=self._set_alias(msg.alias),
            request_id=msg.request_id,
        ))

    async def _handle_exec(self, msg: ExecRequest) -> None:
        assert self._execution_lock is not None
        if self._execution_lock.locked():
            await self._send(ExecResult(
                execution_id=msg.execution_id,
                status=ExecStatus.FAILED,
                stdout="",
                stderr="",
                error=ExecError.BUSY,
                request_id=msg.request_id,
            ))
            return

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
                result.request_id = msg.request_id
            except Exception as exc:
                result = ExecResult(
                    execution_id=msg.execution_id,
                    status=ExecStatus.FAILED,
                    stdout=out.getvalue(),
                    stderr=err.getvalue(),
                    error=str(exc),
                    request_id=msg.request_id,
                )
            finally:
                flusher.stop()
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
