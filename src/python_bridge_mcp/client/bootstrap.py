import asyncio
import sys
import threading
from dataclasses import dataclass
from typing import Callable, Optional, cast

from .code_executor import CodeExecutor
from .code_runner import CodeRunner, DirectRunner
from .discovery import DiscoveryClient
from .exec_listener import ExecListener

_SERVICE_ATTR = "_python_bridge_mcp_listener_service"


@dataclass
class ListenerService:
    listener: ExecListener
    discovery: DiscoveryClient
    listener_thread: threading.Thread
    discovery_thread: threading.Thread

    def is_running(self) -> bool:
        return self.listener_thread.is_alive() and self.discovery_thread.is_alive()

    def stop(self) -> None:
        self.discovery.stop()
        self.listener.stop()
        self.discovery_thread.join(timeout=2)
        self.listener_thread.join(timeout=2)
        if getattr(sys, _SERVICE_ATTR, None) is self:
            delattr(sys, _SERVICE_ATTR)


def get_listener_service() -> Optional[ListenerService]:
    return cast(Optional[ListenerService], getattr(sys, _SERVICE_ATTR, None))


def start_listener_service(
    instance_id: str,
    instance_name: str,
    instance_type: str = "",
    runner: Optional[CodeRunner] = None,
    exec_host: str = "127.0.0.1",
    exec_port: int = 0,
    discovery_host: str = DiscoveryClient.DEFAULT_HOST,
    discovery_port: int = DiscoveryClient.DEFAULT_PORT,
    heartbeat_interval: float = DiscoveryClient.HEARTBEAT_INTERVAL,
    alias: Optional[str] = None,
    alias_getter: Optional[Callable[[], Optional[str]]] = None,
) -> ListenerService:
    # Reload-safe: replace an existing service so code changes pick up immediately.
    current = get_listener_service()
    if current is not None:
        current.stop()

    if runner is None:
        runner = DirectRunner(CodeExecutor())

    listener = ExecListener(exec_host, exec_port, runner)
    if alias is not None:
        listener.set_alias(alias)

    def _run_listener() -> None:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        loop.run_until_complete(listener.run())

    listener_thread = threading.Thread(target=_run_listener, daemon=True)
    listener_thread.start()
    if not listener.wait_started(timeout=5):
        raise RuntimeError("ExecListener failed to start within 5s")

    discovery = DiscoveryClient(
        instance_id=instance_id,
        instance_name=instance_name,
        exec_host=exec_host,
        exec_port=listener.port,
        alias=alias,
        alias_getter=alias_getter if alias_getter is not None else listener.get_alias,
        instance_type=instance_type,
        host=discovery_host,
        port=discovery_port,
        heartbeat_interval=heartbeat_interval,
    )
    discovery_thread = threading.Thread(target=discovery.run, daemon=True)
    discovery_thread.start()

    service = ListenerService(
        listener=listener,
        discovery=discovery,
        listener_thread=listener_thread,
        discovery_thread=discovery_thread,
    )
    setattr(sys, _SERVICE_ATTR, service)
    return service


def stop_listener_service() -> None:
    current = get_listener_service()
    if current is not None:
        current.stop()
