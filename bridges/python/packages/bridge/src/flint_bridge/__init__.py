"""Connect an application's Python runtime to flint from its UI thread."""
import sys
import threading

__version__ = "0.1.0"
_SERVICE = "_flint_bridge_service"
_LOCK = "_flint_bridge_lifecycle_lock"


def connect(host, address="127.0.0.1", port=6321, name=None, enabled=True):
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
        strategy = strategy_for(host)
        try:
            bridge = Bridge(CodeRunner(CodeExecutor(), strategy), host, address, port, name or host, enabled)
            bridge.start()
        except BaseException:
            strategy.close()
            raise
        setattr(sys, _SERVICE, bridge)
        return bridge


def configure(host, address="127.0.0.1", port=6321, name=None, enabled=True):
    """Apply connection settings without rebuilding the host execution adapter."""
    lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
    with lock:
        current = getattr(sys, _SERVICE, None)
        if current is None:
            return connect(host, address, port, name, enabled)
        if current.host != host:
            raise RuntimeError("Disconnect the existing host Bridge before changing host type")
        current.apply_settings(address, port, name or current.name, enabled)
        return current


def current():
    """Return this interpreter's Bridge, if it has been started."""
    return getattr(sys, _SERVICE, None)


def reconnect():
    """Retry the current endpoint without replacing the execution adapter."""
    lock = sys.__dict__.setdefault(_LOCK, threading.RLock())
    with lock:
        bridge = current()
        if bridge is None:
            raise RuntimeError("No Bridge has been started")
        bridge._force_reconnect()
        return bridge


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
