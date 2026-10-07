from ..execution.scheduling import CallbackQueue


class BlenderTimerQueue(CallbackQueue):
    """Drains posted callbacks from a persistent timer on Blender's main thread."""

    def __init__(self, timers):
        super().__init__()
        self._timers = timers
        self._timer = self._tick
        timers.register(self._timer, first_interval=0.0, persistent=True)

    def _tick(self):
        self.drain()
        return 0.02

    def close(self):
        if self._timers.is_registered(self._timer):
            self._timers.unregister(self._timer)
        super().close()
