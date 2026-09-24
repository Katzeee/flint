import platform
import threading
import time

from ..execution.task import ExecutionTask
from .native import NativeCore


class Bridge:
    """Poll the native core and submit each task's events from one Bridge thread."""

    def __init__(self, runner, host, address, port, name, enabled=True):
        self.runner, self.host, self.address, self.port, self.name = runner, host, address, port, name
        self.enabled = enabled
        self._core = NativeCore({
            "host": host,
            "address": address,
            "port": port,
            "name": name,
            "enabled": enabled,
            "runtime_version": "CPython " + platform.python_version(),
        })
        self._stop = threading.Event()
        self.thread = threading.Thread(target=self._run, name="flint-bridge", daemon=True)

    def start(self):
        self.thread.start()
        return self

    @property
    def instance_id(self):
        return self._core.instance_id

    @property
    def connected(self):
        return self._core.connected

    @property
    def busy(self):
        return self._core.busy

    @property
    def status(self):
        return self._core.status

    def wait_until_connected(self, timeout=10):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.connected:
                return True
            time.sleep(0.02)
        return self.connected

    def stop(self, timeout=5):
        self._stop.set()
        self.runner.close()
        self._core.stop()
        if threading.current_thread() is not self.thread:
            self.thread.join(timeout)
        if self.thread.is_alive() or self.busy:
            return False
        self._core.close()
        return True

    def _force_reconnect(self):
        self._core.reconnect()

    def apply_settings(self, address, port, name, enabled=True):
        self._core.apply_settings({
            "address": address, "port": port, "name": name, "enabled": enabled,
        })
        self.address, self.port, self.name, self.enabled = address, port, name, enabled

    def _run(self):
        active = None
        while not self._stop.is_set() or active is not None:
            if self._stop.is_set():
                active.join(0.2)
            else:
                event = self._core.poll(100 if active is not None else 200)
                if event is not None:
                    if self._stop.is_set():
                        self._reject_unstarted(event)
                    elif active is None:
                        active = ExecutionTask(self.runner, event)
                        active.start()
                    else:
                        raise RuntimeError("Native core delivered overlapping executions")
            if active is not None and active.drain(self._core.submit):
                active = None
        pending = self._core.poll(0)
        if pending is not None:
            self._reject_unstarted(pending)

    def _reject_unstarted(self, event):
        self._core.submit({
            "kind": "result", "request_id": event["request_id"],
            "succeeded": False, "traceback": None,
            "error": "Bridge stopped before host execution",
        })
