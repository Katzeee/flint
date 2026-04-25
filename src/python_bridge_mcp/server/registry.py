import asyncio
import logging
import threading
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional
from uuid import uuid4

from ..shared.discovery_models import AckDiscovery, HeartbeatDiscovery, RegisterDiscovery
from ..shared.exec_models import ExecResult, SetAliasResult
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError

log = logging.getLogger(__name__)


@dataclass
class ClientEntry:
    pid: int
    instance_id: str
    instance_name: str
    alias: Optional[str]
    instance_type: str = ""
    last_heartbeat: float = field(default_factory=time.monotonic)


class ControlSession:
    def __init__(self, writer: asyncio.StreamWriter) -> None:
        self._writer = writer
        self._write_lock = asyncio.Lock()
        self._pending: Dict[str, asyncio.Future] = {}
        self._loop = asyncio.get_running_loop()

    async def send(self, msg: VersionedWireModel) -> None:
        async with self._write_lock:
            await AsyncJsonLineCodec.send(self._writer, msg.to_dict())

    def track(self, request_id: str) -> asyncio.Future:
        future = self._loop.create_future()
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
        def _fail() -> None:
            pending = list(self._pending.values())
            self._pending.clear()
            for future in pending:
                if not future.done():
                    future.set_exception(exc)

        if self._loop.is_running():
            self._loop.call_soon_threadsafe(_fail)

    def close(self) -> None:
        def _close() -> None:
            self._writer.close()

        if self._loop.is_running():
            self._loop.call_soon_threadsafe(_close)


class Registry:
    DEFAULT_HOST = "localhost"
    DEFAULT_PORT = 6321
    DEFAULT_STALE_TIMEOUT = 15.0

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
        stale_timeout: float = DEFAULT_STALE_TIMEOUT,
    ):
        self._host = host
        self._port = port
        self._stale_timeout = stale_timeout
        self._clients: Dict[str, ClientEntry] = {}
        self._sessions: Dict[str, ControlSession] = {}
        self._lock = threading.Lock()
        self._server: Optional[asyncio.AbstractServer] = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def get_client(self, instance_id: str) -> Optional[ClientEntry]:
        stale_sessions: List[ControlSession]
        with self._lock:
            stale_sessions = self._evict_stale()
            entry = self._clients.get(instance_id)
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        return entry

    def list_clients(self, instance_type: Optional[str] = None) -> Dict[str, ClientEntry]:
        stale_sessions: List[ControlSession]
        with self._lock:
            stale_sessions = self._evict_stale()
            if instance_type is None:
                result = dict(self._clients)
            else:
                result = {k: v for k, v in self._clients.items() if v.instance_type == instance_type}
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        return result

    def register(self, entry: ClientEntry, session: Optional[ControlSession] = None) -> None:
        old_sessions: List[ControlSession]
        with self._lock:
            old_sessions = self._evict_stale()
            for iid, existing in list(self._clients.items()):
                if existing.pid == entry.pid or iid == entry.instance_id:
                    old_session = self._sessions.pop(iid, None)
                    if old_session is not None:
                        old_sessions.append(old_session)
                    del self._clients[iid]
            self._clients[entry.instance_id] = entry
            if session is not None:
                self._sessions[entry.instance_id] = session
        self._close_sessions(old_sessions, "client replaced by a new session")
        log.info(
            "Registered client %s (pid=%d, type=%s)",
            entry.instance_id, entry.pid, entry.instance_type,
        )

    def unregister(
        self,
        instance_id: str,
        pid: Optional[int] = None,
        session: Optional[ControlSession] = None,
    ) -> None:
        removed = False
        removed_session: Optional[ControlSession] = None
        with self._lock:
            if pid is not None:
                entry = self._clients.get(instance_id)
                if entry is None or entry.pid != pid:
                    return
            if session is not None and self._sessions.get(instance_id) is not session:
                return
            removed = self._clients.pop(instance_id, None) is not None
            removed_session = self._sessions.pop(instance_id, None)
        if removed_session is not None:
            self._close_sessions([removed_session], "client session closed")
        if removed:
            log.info("Unregistered client %s", instance_id)

    def heartbeat(self, instance_id: str) -> bool:
        stale_sessions: List[ControlSession]
        with self._lock:
            stale_sessions = self._evict_stale()
            if instance_id not in self._clients:
                log.debug("Heartbeat from unregistered client %s", instance_id)
                ok = False
            else:
                self._clients[instance_id].last_heartbeat = time.monotonic()
                ok = True
        self._close_sessions(stale_sessions, "client heartbeat timed out")
        return ok

    def set_alias(self, instance_id: str, alias: Optional[str]) -> None:
        stale_sessions: List[ControlSession]
        with self._lock:
            stale_sessions = self._evict_stale()
            if instance_id not in self._clients:
                raise KeyError(f"unknown client: {instance_id}")
            self._clients[instance_id].alias = alias if alias and alias.strip() else None
        self._close_sessions(stale_sessions, "client heartbeat timed out")

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

        with self._lock:
            stale_sessions = self._evict_stale()
            session = self._sessions.get(instance_id)
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

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _evict_stale(self) -> List[ControlSession]:
        """Remove clients whose last heartbeat exceeds stale_timeout. Must be called with lock held."""
        now = time.monotonic()
        stale = [
            iid for iid, entry in self._clients.items()
            if now - entry.last_heartbeat > self._stale_timeout
        ]
        sessions: List[ControlSession] = []
        for iid in stale:
            del self._clients[iid]
            session = self._sessions.pop(iid, None)
            if session is not None:
                sessions.append(session)
        return sessions

    @staticmethod
    def _close_sessions(sessions: List[ControlSession], reason: str) -> None:
        for session in sessions:
            session.fail_pending(ConnectionError(reason))
            session.close()

    async def run(self) -> None:
        self._server = await asyncio.start_server(
            self._handle_client, self._host, self._port
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
            sessions: List[ControlSession]
            with self._lock:
                evicted = self._evict_stale()
                sessions = evicted
            if sessions:
                self._close_sessions(sessions, "client heartbeat timed out")
                log.info("Evicted %d stale client session(s)", len(sessions))

    def stop(self) -> None:
        if self._server is not None:
            self._server.close()

    # ------------------------------------------------------------------
    # Connection handler
    # ------------------------------------------------------------------

    async def _handle_client(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        instance_id: Optional[str] = None
        registered_pid: Optional[int] = None
        session: Optional[ControlSession] = None
        try:
            while True:
                try:
                    data = await asyncio.wait_for(AsyncJsonLineCodec.recv(reader), timeout=30)
                    msg = VersionedWireModel.parse_versioned(data)
                except asyncio.TimeoutError:
                    return  # no heartbeat in time — drop connection
                except (ConnectionError, WireModelError, ValueError) as e:
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error=str(e)).to_dict())
                    return

                if isinstance(msg, RegisterDiscovery):
                    if session is not None:
                        await session.send(AckDiscovery(success=False, error="already registered"))
                        return
                    instance_id = msg.instance_id
                    registered_pid = msg.pid
                    session = ControlSession(writer)
                    self.register(ClientEntry(
                        pid=msg.pid,
                        instance_id=msg.instance_id,
                        instance_name=msg.instance_name,
                        alias=msg.alias,
                        instance_type=msg.instance_type,
                    ), session)
                    await session.send(AckDiscovery(success=True))

                elif isinstance(msg, HeartbeatDiscovery):
                    if session is None:
                        await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error="not registered").to_dict())
                        return
                    if not self.heartbeat(msg.instance_id):
                        await session.send(AckDiscovery(success=False, error="not registered"))
                        return
                    await session.send(AckDiscovery(success=True))

                elif isinstance(msg, (ExecResult, SetAliasResult)):
                    if session is None or not session.resolve(msg):
                        log.warning("Dropping unmatched response from %s: %s", instance_id, type(msg).__name__)

                else:
                    if session is not None:
                        await session.send(AckDiscovery(success=False, error="unexpected message type"))
                    else:
                        await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error="unexpected message type").to_dict())
                    return
        finally:
            if instance_id is not None:
                # Only unregister if this connection's PID still owns the entry.
                # A re-registration with a new PID must not be evicted by the
                # stale connection's close.
                self.unregister(instance_id, pid=registered_pid, session=session)
            writer.close()
