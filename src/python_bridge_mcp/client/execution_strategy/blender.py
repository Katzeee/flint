import threading
from queue import Empty, Queue
from typing import Any, Callable, Optional

from .base import ResultT
from .queued import QueuedExecutionStrategy


class BlenderMainThreadExecutionStrategy(QueuedExecutionStrategy):
    """Runs queued work from Blender's main thread via ``bpy.app.timers``."""

    def __init__(
        self,
        bpy_module: Optional[Any] = None,
        interval: float = 0.01,
        max_tasks_per_tick: int = 8,
    ) -> None:
        if bpy_module is None:
            import bpy as bpy_module  # pyright: ignore[reportMissingImports]

        if threading.current_thread() is not threading.main_thread():
            raise RuntimeError("Blender execution strategy must be created on the main thread")
        if max_tasks_per_tick < 1:
            raise ValueError("max_tasks_per_tick must be at least 1")

        self._bpy = bpy_module
        self._interval = interval
        self._max_tasks_per_tick = max_tasks_per_tick
        self._owned_queue: Queue = Queue()
        self._timer_callback = self._pump
        super().__init__(self._owned_queue)
        self._bpy.app.timers.register(
            self._timer_callback,
            first_interval=0.0,
            persistent=True,
        )

    def run(self, func: Callable[[], ResultT]) -> ResultT:
        if threading.current_thread() is threading.main_thread():
            with self._admit():
                pass
            return func()
        return super().run(func)

    def _close(self) -> None:
        super()._close()

        if threading.current_thread() is threading.main_thread():
            timers = self._bpy.app.timers
            if timers.is_registered(self._timer_callback):
                timers.unregister(self._timer_callback)

    def _pump(self) -> Optional[float]:
        if self._is_closed():
            return None

        for _index in range(self._max_tasks_per_tick):
            try:
                invocation = self._owned_queue.get_nowait()
            except Empty:
                break
            invocation()
        return self._interval
