import asyncio
import logging
import threading
import time
from dataclasses import dataclass, field
from typing import Dict, List, Optional

from ..shared.discovery_models import AckDiscovery, HeartbeatDiscovery, RegisterDiscovery
from ..shared.jsonline import AsyncJsonLineCodec
from ..shared.model_base import VersionedWireModel, WireModelError

log = logging.getLogger(__name__)


@dataclass
class ClientEntry:
    pid: int
    instance_id: str
    instance_name: str
    exec_host: str
    exec_port: int
    alias: Optional[str]
    instance_type: str = ""
    last_heartbeat: float = field(default_factory=time.monotonic)


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
        self._lock = threading.Lock()
        self._server: Optional[asyncio.AbstractServer] = None

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def get_client(self, instance_id: str) -> Optional[ClientEntry]:
        with self._lock:
            self._evict_stale()
            return self._clients.get(instance_id)

    def list_clients(self, instance_type: Optional[str] = None) -> Dict[str, ClientEntry]:
        with self._lock:
            self._evict_stale()
            if instance_type is None:
                return dict(self._clients)
            return {k: v for k, v in self._clients.items() if v.instance_type == instance_type}

    def register(self, entry: ClientEntry) -> None:
        with self._lock:
            self._evict_stale()
            self._clients = {iid: e for iid, e in self._clients.items() if e.pid != entry.pid}
            self._clients[entry.instance_id] = entry
        log.info(
            "Registered client %s (pid=%d, type=%s, exec=%s:%d)",
            entry.instance_id, entry.pid, entry.instance_type, entry.exec_host, entry.exec_port,
        )

    def unregister(self, instance_id: str, pid: Optional[int] = None) -> None:
        removed = False
        with self._lock:
            self._evict_stale()
            if pid is not None:
                entry = self._clients.get(instance_id)
                if entry is None or entry.pid != pid:
                    return
            removed = self._clients.pop(instance_id, None) is not None
        if removed:
            log.info("Unregistered client %s", instance_id)

    def heartbeat(self, instance_id: str) -> bool:
        with self._lock:
            self._evict_stale()
            if instance_id not in self._clients:
                log.debug("Heartbeat from unregistered client %s", instance_id)
                return False
            self._clients[instance_id].last_heartbeat = time.monotonic()
            return True

    def set_alias(self, instance_id: str, alias: Optional[str]) -> None:
        with self._lock:
            self._evict_stale()
            if instance_id not in self._clients:
                raise KeyError(f"unknown client: {instance_id}")
            self._clients[instance_id].alias = alias if alias and alias.strip() else None

    # ------------------------------------------------------------------
    # Internal
    # ------------------------------------------------------------------

    def _evict_stale(self) -> List[str]:
        """Remove clients whose last heartbeat exceeds stale_timeout. Must be called with lock held."""
        now = time.monotonic()
        stale = [
            iid for iid, entry in self._clients.items()
            if now - entry.last_heartbeat > self._stale_timeout
        ]
        for iid in stale:
            del self._clients[iid]
        return stale

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
            with self._lock:
                evicted = self._evict_stale()
            if evicted:
                log.info("Evicted %d stale client(s): %s", len(evicted), evicted)

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
                    instance_id = msg.instance_id
                    registered_pid = msg.pid
                    self.register(ClientEntry(
                        pid=msg.pid,
                        instance_id=msg.instance_id,
                        instance_name=msg.instance_name,
                        exec_host=msg.exec_host,
                        exec_port=msg.exec_port,
                        alias=msg.alias,
                        instance_type=msg.instance_type,
                    ))
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=True).to_dict())

                elif isinstance(msg, HeartbeatDiscovery):
                    if not self.heartbeat(msg.instance_id):
                        await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error="not registered").to_dict())
                        return
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=True).to_dict())

                else:
                    await AsyncJsonLineCodec.send(writer, AckDiscovery(success=False, error="unexpected message type").to_dict())
                    return
        finally:
            if instance_id is not None:
                # Only unregister if this connection's PID still owns the entry.
                # A re-registration with a new PID must not be evicted by the
                # stale connection's close.
                self.unregister(instance_id, pid=registered_pid)
            writer.close()
