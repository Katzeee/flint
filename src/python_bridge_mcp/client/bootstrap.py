import logging
import sys
import threading
from dataclasses import dataclass
from typing import Optional, cast

from ..shared.constants import DEFAULT_HOST, REGISTRY_PORT
from .code_runner import CodeRunner
from .discovery import DiscoveryClient

_SERVICE_ATTR = "_python_bridge_mcp_control_client_service"
_LIFECYCLE_LOCK_ATTR = "_python_bridge_mcp_control_client_lifecycle_lock"
_STOP_TIMEOUT = 5.0

log = logging.getLogger(__name__)


def _get_lifecycle_lock() -> threading.RLock:
    """Return a process-wide lock that survives module reloads."""
    return cast(
        threading.RLock,
        sys.__dict__.setdefault(_LIFECYCLE_LOCK_ATTR, threading.RLock()),
    )


@dataclass
class ControlClientService:
    client: DiscoveryClient
    thread: threading.Thread

    def is_running(self) -> bool:
        return self.thread.is_alive()

    def stop(self, timeout: float = _STOP_TIMEOUT) -> bool:
        """Stop the discovery thread.

        The process-wide service reference is retained if the thread does not
        exit, preventing a reload from starting a second discovery listener.
        """
        with _get_lifecycle_lock():
            self.client.stop()
            if threading.current_thread() is self.thread:
                log.error("Discovery service cannot join its own thread")
                return False
            self.thread.join(timeout=timeout)
            if self.thread.is_alive():
                log.error(
                    "Discovery service thread did not stop within %.1fs; "
                    "refusing to discard its service reference",
                    timeout,
                )
                return False
            if getattr(sys, _SERVICE_ATTR, None) is self:
                delattr(sys, _SERVICE_ATTR)
            return True


def get_control_client_service() -> Optional[ControlClientService]:
    return cast(Optional[ControlClientService], getattr(sys, _SERVICE_ATTR, None))


def start_control_client_service(
    name_hint: str,
    instance_name: str,
    runner: CodeRunner,
    instance_type: str = "",
    discovery_host: str = DEFAULT_HOST,
    discovery_port: int = REGISTRY_PORT,
    heartbeat_interval: float = DiscoveryClient.HEARTBEAT_INTERVAL,
) -> ControlClientService:
    if runner is None:
        raise TypeError("runner is required")

    # The lock lives on sys so concurrent calls and module reloads share the
    # same lifecycle boundary.
    with _get_lifecycle_lock():
        current = get_control_client_service()
        if current is not None:
            current.stop()
            if current.is_running():
                # Also repairs the reference if an older, pre-reload service
                # implementation discarded it before verifying thread exit.
                setattr(sys, _SERVICE_ATTR, current)
                raise RuntimeError(
                    "Existing discovery service did not stop; "
                    "refusing to start a second listener"
                )

        client = DiscoveryClient(
            name_hint=name_hint,
            instance_name=instance_name,
            runner=runner,
            instance_type=instance_type,
            host=discovery_host,
            port=discovery_port,
            heartbeat_interval=heartbeat_interval,
        )
        thread = threading.Thread(
            target=client.run,
            name="python-bridge-mcp-discovery",
            daemon=True,
        )
        thread.start()

        service = ControlClientService(client=client, thread=thread)
        setattr(sys, _SERVICE_ATTR, service)
        return service


def stop_control_client_service() -> bool:
    with _get_lifecycle_lock():
        current = get_control_client_service()
        if current is None:
            return True
        return current.stop()
