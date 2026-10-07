"""A host's Bridge: its native core and the scheduler it runs callbacks on."""
import platform
import time
from .native_core import NativeCore


class Bridge:
    def __init__(self, capabilities, host, address, port, name, enabled=True):
        self.host = host
        self._scheduler = capabilities.scheduler
        try:
            self._core = NativeCore({
                "host": host, "address": address, "port": port, "name": name, "enabled": enabled,
                "runtime_version": platform.python_implementation() + " " + platform.python_version(),
            }, capabilities)
        except BaseException:
            self._scheduler.close()
            raise

    @property
    def address(self):
        return self.status["settings"]["address"]

    @property
    def port(self):
        return self.status["settings"]["port"]

    @property
    def name(self):
        return self.status["settings"]["name"]

    @property
    def enabled(self):
        return self.status["settings"]["enabled"]

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

    def stop(self):
        """Release the Bridge once no started host code remains; False means retry later."""
        if not self._core.stop():
            return False
        self._core.destroy()
        self._scheduler.close()
        return True

    def _force_reconnect(self):
        self._core.reconnect()

    def check_running(self):
        self._core.check_running()

    def apply_settings(self, address, port, name, enabled=True):
        self._core.apply_settings({"address": address, "port": port, "name": name, "enabled": enabled})
