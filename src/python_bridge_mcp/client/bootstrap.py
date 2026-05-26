import sys
import threading
from dataclasses import dataclass
from typing import Callable, Optional, cast

from ..shared.constants import DEFAULT_HOST, REGISTRY_PORT
from .code_runner import CodeRunner
from .discovery import DiscoveryClient

_SERVICE_ATTR = "_python_bridge_mcp_control_client_service"


@dataclass
class ControlClientService:
    client: DiscoveryClient
    thread: threading.Thread

    def is_running(self) -> bool:
        return self.thread.is_alive()

    def stop(self) -> None:
        self.client.stop()
        self.thread.join(timeout=2)
        if getattr(sys, _SERVICE_ATTR, None) is self:
            delattr(sys, _SERVICE_ATTR)


def get_control_client_service() -> Optional[ControlClientService]:
    return cast(Optional[ControlClientService], getattr(sys, _SERVICE_ATTR, None))


def start_control_client_service(
    instance_id: str,
    instance_name: str,
    instance_type: str = "",
    runner: Optional[CodeRunner] = None,
    discovery_host: str = DEFAULT_HOST,
    discovery_port: int = REGISTRY_PORT,
    heartbeat_interval: float = DiscoveryClient.HEARTBEAT_INTERVAL,
    alias: Optional[str] = None,
    alias_getter: Optional[Callable[[], Optional[str]]] = None,
) -> ControlClientService:
    # Reload-safe: replace an existing service so code changes pick up immediately.
    current = get_control_client_service()
    if current is not None:
        current.stop()

    if runner is None:
        runner = CodeRunner.create_default()

    client = DiscoveryClient(
        instance_id=instance_id,
        instance_name=instance_name,
        runner=runner,
        alias=alias,
        alias_getter=alias_getter,
        instance_type=instance_type,
        host=discovery_host,
        port=discovery_port,
        heartbeat_interval=heartbeat_interval,
    )
    thread = threading.Thread(target=client.run, daemon=True)
    thread.start()

    service = ControlClientService(client=client, thread=thread)
    setattr(sys, _SERVICE_ATTR, service)
    return service


def stop_control_client_service() -> None:
    current = get_control_client_service()
    if current is not None:
        current.stop()
