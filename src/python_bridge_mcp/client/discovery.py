from __future__ import annotations

import logging
import os
import socket
import threading
from enum import Enum
from typing import Callable, Optional, Union

from ..shared.discovery_models import AckDiscovery, HeartbeatDiscovery, RegisterDiscovery
from ..shared.jsonline import SyncJsonLineCodec
from ..shared.model_base import VersionedWireModel

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

    def __init__(
        self,
        instance_id: str,
        instance_name: str,
        exec_host: str,
        exec_port: int,
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
        self._exec_host = exec_host
        self._exec_port = exec_port
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
                self._connect_and_heartbeat()
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
        self._set_state(DiscoveryState.STOPPED)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _current_alias(self) -> Optional[str]:
        if self._alias_getter is not None:
            alias = self._alias_getter()
        else:
            alias = self._alias
        alias = alias.strip() if alias is not None else None
        return alias or None

    def _connect_and_heartbeat(self) -> None:
        with socket.create_connection((self._host, self._port), timeout=10) as conn:
            conn.settimeout(self._heartbeat_interval + 5)

            # Register
            self._send_and_check(conn, RegisterDiscovery(
                pid=self._pid,
                instance_id=self._instance_id,
                instance_name=self._instance_name,
                exec_host=self._exec_host,
                exec_port=self._exec_port,
                alias=self._current_alias(),
                instance_type=self._instance_type,
            ), "Registration rejected")
            self._set_state(DiscoveryState.CONNECTED)
            log.info(
                "Discovery connected to %s:%d as %s",
                self._host, self._port, self._instance_id,
            )

            # Heartbeat loop
            while not self._stop_event.is_set():
                self._stop_event.wait(self._heartbeat_interval)
                if self._stop_event.is_set():
                    break
                self._send_and_check(conn, HeartbeatDiscovery(
                    instance_id=self._instance_id,
                ), "Heartbeat rejected")

    @staticmethod
    def _send_and_check(conn: socket.socket, msg: VersionedWireModel, context: str) -> None:
        SyncJsonLineCodec.send(conn, msg.to_dict())
        ack = VersionedWireModel.parse_versioned(SyncJsonLineCodec.recv(conn))
        if not isinstance(ack, AckDiscovery):
            raise RuntimeError(f"{context}: unexpected response")
        if not ack.success:
            raise RuntimeError(f"{context}: {ack.error}")

    def _set_state(self, state: DiscoveryState) -> None:
        with self._state_lock:
            self._state = state
        if state == DiscoveryState.CONNECTED:
            self._connected_event.set()
        else:
            self._connected_event.clear()
