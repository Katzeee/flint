import threading
from typing import Any, Callable, Optional

from ._queued_dispatcher import QueuedDispatcher
from .base import ExecutionStrategy, ResultT


class BlenderMainThreadExecutionStrategy(ExecutionStrategy):
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

        super().__init__()
        self._bpy = bpy_module
        self._interval = interval
        self._max_tasks_per_tick = max_tasks_per_tick
        self._dispatcher = QueuedDispatcher()
        self._timer_callback = self._pump
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
        with self._admit():
            invocation = self._dispatcher.submit(func)
        return invocation.wait()

    def _close(self) -> None:
        self._dispatcher.cancel_pending()

        if threading.current_thread() is threading.main_thread():
            timers = self._bpy.app.timers
            if timers.is_registered(self._timer_callback):
                timers.unregister(self._timer_callback)

    def _pump(self) -> Optional[float]:
        if self._is_closed():
            return None

        self._dispatcher.execute_pending(self._max_tasks_per_tick)
        return self._interval
