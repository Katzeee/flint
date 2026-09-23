"""Connect an application's Python runtime to flint from its UI thread."""
import sys
import threading

__version__ = "0.1.0"
_SERVICE = "_flint_bridge_service"
_LOCK = "_flint_bridge_lifecycle_lock"


def connect(host, address="127.0.0.1", port=6321, name=None):
    """Return the existing matching bridge, or start a new bridge for this host."""
    lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
    with lock:
        current = getattr(sys, _SERVICE, None)
        if current is not None:
            if (current.host, current.address, current.port) != (host, address, port):
                raise RuntimeError("Disconnect the existing bridge before changing its endpoint")
            if current.thread.is_alive():
                return current
            if current.busy:
                raise RuntimeError("Previous host execution is still active")
        from .hosts import strategy_for
        from .execution.executor import CodeExecutor
        from .execution.runner import CodeRunner
        from .connection.service import Bridge
        bridge = Bridge(CodeRunner(CodeExecutor(), strategy_for(host)), host, address, port, name or host)
        setattr(sys, _SERVICE, bridge)
        return bridge.start()


def disconnect():
    """Stop transport and release dispatch resources; running code is not killed."""
    lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
    with lock:
        current = getattr(sys, _SERVICE, None)
        if current is None:
            return True
        if not current.stop():
            return False
        delattr(sys, _SERVICE)
        return True
