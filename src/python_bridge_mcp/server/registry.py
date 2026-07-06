import asyncio
import logging
import time
from dataclasses import dataclass, field
from typing import Callable, Dict, List, Optional
from uuid import uuid4

from ..shared.instance_control_models import (
    InstanceExecOutputUpdate,
    InstanceExecRegister,
    InstanceExecResult,
    InstanceAck,
    InstanceControlError,
    InstanceHeartbeat,
    InstanceRegister,
)
from ..shared.constants import DEFAULT_HOST, REGISTRY_PORT
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError

log = logging.getLogger(__name__)


@dataclass
class ClientEntry:
    pid: int
    instance_id: str
    instance_name: str
    instance_type: str = ""
    last_heartbeat: float = field(default_factory=time.monotonic)


class ClientSession:
    def __init__(self, writer: asyncio.StreamWriter) -> None:
        self._writer = writer
        self._write_lock = asyncio.Lock()
        self._pending: Dict[str, asyncio.Future] = {}

    async def send(self, msg: VersionedWireModel) -> None:
        async with self._write_lock:
            await AsyncJsonLineCodec.send(self._writer, msg.to_dict())

    def track(self, request_id: str) -> asyncio.Future:
        future = asyncio.get_running_loop().create_future()
        self._pending[request_id] = future
        return future

    def forget(self, request_id: str) -> None:
        self._pending.pop(request_id, None)

    def resolve(self, msg: VersionedWireModel) -> bool:
        request_id = getattr(msg, "request_id", None)
        if not request_id:
            return False
        future = self._pending.pop(request_id, None)
        if future is None or future.done():
            return False
        future.set_result(msg)
        return True

    def fail_pending(self, exc: Exception) -> None:
        pending = list(self._pending.values())
        self._pending.clear()
        for future in pending:
            if not future.done():
                future.set_exception(exc)

    def close(self) -> None:
        self._writer.close()


class Registry:
    DEFAULT_STALE_TIMEOUT = 15.0

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = REGISTRY_PORT,
        stale_timeout: float = DEFAULT_STALE_TIMEOUT,
    ):
        self._host = host
        self._port = port
        self._stale_timeout = stale_timeout
        self._clients: Dict[str, ClientEntry] = {}
        self._control_sessions: Dict[str, ClientSession] = {}
        self._exec_sessions: Dict[str, ClientSession] = {}
        self._output_update_handler: Optional[Callable[[InstanceExecOutputUpdate], None]] = None
        self._server: Optional[asyncio.AbstractServer] = None
        self._id_counter: int = 0

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def get_client(self, instance_id: str) -> Optional[ClientEntry]:
        stale_sessions = self._evict_stale()
        entry = self._clients.get(instance_id)
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        return entry

    def list_clients(self, instance_type: Optional[str] = None) -> Dict[str, ClientEntry]:
        stale_sessions = self._evict_stale()
        if instance_type is None:
            result = dict(self._clients)
        else:
            result = {k: v for k, v in self._clients.items() if v.instance_type == instance_type}
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        return result

    def register(self, entry: ClientEntry, control_session: Optional[ClientSession] = None) -> None:
        old_sessions = self._evict_stale()
        for iid, existing in list(self._clients.items()):
            if existing.pid == entry.pid or iid == entry.instance_id:
                ctrl = self._control_sessions.pop(iid, None)
                ex = self._exec_sessions.pop(iid, None)
                if ctrl is not None:
                    old_sessions.append(ctrl)
                if ex is not None:
                    old_sessions.append(ex)
                del self._clients[iid]
        self._clients[entry.instance_id] = entry
        if control_session is not None:
            self._control_sessions[entry.instance_id] = control_session
        self._close_sessions(old_sessions, "client replaced by a new session")
        log.info(
            "Registered client %s (pid=%d, type=%s)",
            entry.instance_id, entry.pid, entry.instance_type,
        )

    def unregister(self, instance_id: str, pid: Optional[int] = None) -> None:
        if pid is not None:
            entry = self._clients.get(instance_id)
            if entry is None or entry.pid != pid:
                return
        removed = self._clients.pop(instance_id, None) is not None
        ctrl = self._control_sessions.pop(instance_id, None)
        ex = self._exec_sessions.pop(instance_id, None)
        sessions = [s for s in (ctrl, ex) if s is not None]
        if sessions:
            self._close_sessions(sessions, "client session closed")
        if removed:
            log.info("Unregistered client %s", instance_id)

    def heartbeat(self, instance_id: str) -> bool:
        stale_sessions = self._evict_stale()
        if instance_id not in self._clients:
            log.debug("Heartbeat from unregistered client %s", instance_id)
            ok = False
        else:
            self._clients[instance_id].last_heartbeat = time.monotonic()
            ok = True
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        return ok

    async def request(
        self,
        instance_id: str,
        msg: VersionedWireModel,
        timeout: float,
    ) -> VersionedWireModel:
        request_id = getattr(msg, "request_id", None) or uuid4().hex
        if not hasattr(msg, "request_id"):
            raise WireModelError(f"message type cannot be correlated: {type(msg).__name__}")
        setattr(msg, "request_id", request_id)

        stale_sessions = self._evict_stale()
        session = self._exec_sessions.get(instance_id)
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        if session is None:
            raise KeyError(f"unknown client: {instance_id}")

        future = session.track(request_id)
        try:
            await session.send(msg)
            return await asyncio.wait_for(future, timeout=timeout)
        except Exception:
            session.forget(request_id)
            raise

    def set_output_update_handler(self, handler: Callable[[InstanceExecOutputUpdate], None]) -> None:
        self._output_update_handler = handler

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _assign_instance_id(self, name_hint: str) -> str:
        self._id_counter += 1
        return f"{name_hint}-{self._id_counter:04d}"

    def _evict_stale(self) -> List[ClientSession]:
        """Remove clients whose last heartbeat exceeds stale_timeout. Must be called with lock held."""
        now = time.monotonic()
        stale = [
            iid for iid, entry in self._clients.items()
            if now - entry.last_heartbeat > self._stale_timeout
        ]
        sessions: List[ClientSession] = []
        for iid in stale:
            del self._clients[iid]
            ctrl = self._control_sessions.pop(iid, None)
            ex = self._exec_sessions.pop(iid, None)
            if ctrl is not None:
                sessions.append(ctrl)
            if ex is not None:
                sessions.append(ex)
        return sessions

    @staticmethod
    def _close_sessions(sessions: List[ClientSession], reason: str) -> None:
        for session in sessions:
            session.fail_pending(ConnectionError(reason))
            session.close()

    async def run(self) -> None:
        self._server = await asyncio.start_server(
            self._handle_connection, self._host, self._port
        )
        log.info("Registry listening on %s:%d", self._host, self._port)
        async with self._server:
            evict_task = asyncio.create_task(self._evict_periodically())
            try:
                await self._server.serve_forever()
            finally:
                evict_task.cancel()
                try:
                    await evict_task
                except asyncio.CancelledError:
                    pass

    async def _evict_periodically(self) -> None:
        """Background task: evict stale entries every stale_timeout/2 seconds."""
        interval = max(self._stale_timeout / 2, 1.0)
        while True:
            await asyncio.sleep(interval)
            sessions = self._evict_stale()
            if sessions:
                self._close_sessions(sessions, "client heartbeat timed out")
                log.info("Evicted %d stale client session(s)", len(sessions))

    def stop(self) -> None:
        if self._server is not None:
            self._server.close()

    # ------------------------------------------------------------------
    # Connection handlers
    # ------------------------------------------------------------------

    async def _handle_connection(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        try:
            data = await asyncio.wait_for(AsyncJsonLineCodec.recv(reader), timeout=30)
            msg = VersionedWireModel.parse_versioned(data)
        except asyncio.TimeoutError:
            writer.close()
            return
        except (ConnectionError, WireModelError, ValueError) as e:
            await AsyncJsonLineCodec.send(
                writer,
                InstanceAck(
                    success=False,
                    error_code=InstanceControlError.PROTOCOL_ERROR,
                    message=str(e),
                ).to_dict(),
            )
            writer.close()
            return

        if isinstance(msg, InstanceRegister):
            await self._handle_control_connection(reader, writer, msg)
        elif isinstance(msg, InstanceExecRegister):
            await self._handle_exec_connection(reader, writer, msg)
        else:
            await AsyncJsonLineCodec.send(
                writer,
                InstanceAck(
                    success=False,
                    error_code=InstanceControlError.UNEXPECTED_MESSAGE_TYPE,
                ).to_dict(),
            )
            writer.close()

    async def _handle_control_connection(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        register_msg: InstanceRegister,
    ) -> None:
        instance_id: Optional[str] = None
        registered_pid: Optional[int] = None
        session: Optional[ClientSession] = None
        try:
            assigned_id = self._assign_instance_id(register_msg.name_hint)
            instance_id = assigned_id
            registered_pid = register_msg.pid
            session = ClientSession(writer)
            self.register(
                ClientEntry(
                    pid=register_msg.pid,
                    instance_id=assigned_id,
                    instance_name=register_msg.instance_name,
                    instance_type=register_msg.instance_type,
                ),
                control_session=session,
            )
            await session.send(InstanceAck(success=True, instance_id=assigned_id))

            while True:
                try:
                    data = await asyncio.wait_for(AsyncJsonLineCodec.recv(reader), timeout=30)
                    msg = VersionedWireModel.parse_versioned(data)
                except asyncio.TimeoutError:
                    return  # no heartbeat in time — drop connection
                except (ConnectionError, WireModelError, ValueError) as e:
                    await AsyncJsonLineCodec.send(
                        writer,
                        InstanceAck(
                            success=False,
                            error_code=InstanceControlError.PROTOCOL_ERROR,
                            message=str(e),
                        ).to_dict(),
                    )
                    return

                if isinstance(msg, InstanceHeartbeat):
                    if not self.heartbeat(msg.instance_id):
                        await session.send(InstanceAck(
                            success=False,
                            error_code=InstanceControlError.NOT_REGISTERED,
                        ))
                        return
                    await session.send(InstanceAck(success=True))
                else:
                    await session.send(InstanceAck(
                        success=False,
                        error_code=InstanceControlError.ALREADY_REGISTERED
                        if isinstance(msg, InstanceRegister)
                        else InstanceControlError.UNEXPECTED_MESSAGE_TYPE,
                    ))
                    return
        finally:
            if instance_id is not None:
                # Only unregister if this connection's PID still owns the entry;
                # a re-registration with a new PID must not be evicted by the
                # stale connection's close.
                self.unregister(instance_id, pid=registered_pid)
            if session is None:
                # Registered connections are torn down via unregister()/session.close();
                # only close directly when registration never completed.
                writer.close()

    async def _handle_exec_connection(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        handshake: InstanceExecRegister,
    ) -> None:
        instance_id = handshake.instance_id
        session: Optional[ClientSession] = None
        try:
            entry = self._clients.get(instance_id)
            if entry is None or entry.pid != handshake.pid:
                await AsyncJsonLineCodec.send(
                    writer,
                    InstanceAck(
                        success=False,
                        error_code=InstanceControlError.NOT_REGISTERED,
                        message="instance not registered or pid mismatch",
                    ).to_dict(),
                )
                return
            session = ClientSession(writer)
            self._exec_sessions[instance_id] = session
            await session.send(InstanceAck(success=True))

            while True:
                # No recv timeout: the exec channel is idle between executions;
                # liveness is driven by the control connection's heartbeats.
                data = await AsyncJsonLineCodec.recv(reader)
                msg = VersionedWireModel.parse_versioned(data)

                if isinstance(msg, InstanceExecResult):
                    if not session.resolve(msg):
                        log.warning(
                            "Dropping unmatched response from %s: %s",
                            instance_id, type(msg).__name__,
                        )
                elif isinstance(msg, InstanceExecOutputUpdate):
                    if self._output_update_handler is not None:
                        try:
                            self._output_update_handler(msg)
                        except Exception:
                            log.warning(
                                "Failed to handle output update for %s/%s",
                                msg.workflow_id, msg.execution_id,
                                exc_info=True,
                            )
                else:
                    await session.send(InstanceAck(
                        success=False,
                        error_code=InstanceControlError.UNEXPECTED_MESSAGE_TYPE,
                    ))
                    return
        except (ConnectionError, WireModelError, ValueError) as e:
            log.debug("Exec connection for %s ended: %s", instance_id, e)
        finally:
            if session is not None:
                # Only remove if this session still owns the slot (may have been
                # replaced by a newer exec connection or popped by unregister()).
                if self._exec_sessions.get(instance_id) is session:
                    del self._exec_sessions[instance_id]
                session.fail_pending(ConnectionError("exec session closed"))
                session.close()
            else:
                writer.close()
