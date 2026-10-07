"""Manage a host's Bridge using the execution and scheduling it supplies."""
from dataclasses import dataclass
from concurrent.futures import Future, TimeoutError as WaitTimeout
import sys
import threading
import time
from typing import Callable

from ..execution.capabilities import ExecutionCapabilities
from .bridge import Bridge

_SERVICE = "_flint_bridge_service"
_LOCK = "_flint_bridge_lifecycle_lock"


@dataclass(frozen=True)
class BridgeManager:
    """Manage a fixed host's Bridge; construction does not start a connection."""

    host: str
    create_execution: Callable[[], ExecutionCapabilities]
    dispatch_initialization: Callable[[Callable[[], None]], None]

    def connect(self, address="127.0.0.1", port=6321, name=None, enabled=True):
        """Reuse a matching Bridge or bind this host's execution capabilities."""
        lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
        with lock:
            current = getattr(sys, _SERVICE, None)
            if current is not None:
                current.check_running()
                if (current.host, current.address, current.port) != (self.host, address, port):
                    raise RuntimeError("Disconnect the existing bridge before changing its endpoint or host")
                return current
            capabilities = self.create_execution()
            bridge = Bridge(capabilities, self.host, address, port, self.host if name is None else name, enabled)
            setattr(sys, _SERVICE, bridge)
            return bridge

    def configure(self, address="127.0.0.1", port=6321, name=None, enabled=True):
        """Apply settings without replacing the existing execution adapter."""
        lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
        with lock:
            current = getattr(sys, _SERVICE, None)
            if current is None:
                return self.connect(address, port, name, enabled)
            if current.host != self.host:
                raise RuntimeError("Disconnect the existing host Bridge before changing host type")
            current.apply_settings(address, port, current.name if name is None else name, enabled)
            return current

    def attach(self, address="127.0.0.1", port=6321, name=None, enabled=True, timeout=20):
        """Schedule connection on the host thread and wait for registration.

        Call from a worker thread when the host scheduler queues work onto its
        main thread. Errors propagate to the caller of attach.
        """
        deadline = time.monotonic() + timeout
        attempt = Future()

        def establish():
            if not attempt.set_running_or_notify_cancel():
                return
            try:
                bridge = self.configure(address, port, name, enabled)
            except BaseException as error:
                attempt.set_exception(error)
            else:
                attempt.set_result(bridge)

        try:
            self.dispatch_initialization(establish)
        except BaseException:
            attempt.cancel()
            raise
        try:
            bridge = attempt.result(max(0, deadline - time.monotonic()))
        except WaitTimeout:
            if attempt.done():
                # Propagate a TimeoutError raised by initialization itself, or
                # consume a result completed at the waiting deadline.
                bridge = attempt.result()
            elif attempt.cancel():
                raise TimeoutError("Bridge initialization timed out and was cancelled before starting") from None
            else:
                raise TimeoutError("Bridge initialization is still in progress; connection outcome is unknown") from None
        if enabled and not bridge.wait_until_connected(max(0, deadline - time.monotonic())):
            obstacle = bridge.status["connection"].get("obstacle")
            raise RuntimeError("Bridge registration did not complete" + (
                ": " + obstacle["message"] if obstacle else ""))
        return bridge.instance_id

    def current(self):
        """Return this host's Bridge in the current interpreter, if present."""
        bridge = getattr(sys, _SERVICE, None)
        return bridge if bridge is not None and bridge.host == self.host else None

    def reconnect(self):
        lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
        with lock:
            bridge = self.current()
            if bridge is None:
                raise RuntimeError("No Bridge has been started for this host")
            bridge._force_reconnect()
            return bridge

    def disconnect(self):
        """Release this host's Bridge; running code must finish before disposal."""
        lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
        with lock:
            bridge = self.current()
            if bridge is None:
                return True
            if not bridge.stop():
                return False
            delattr(sys, _SERVICE)
            return True
