from collections import deque
from typing import Any, Callable, Deque, Optional

from ._invocation import Invocation
from .base import ExecutionStrategy, ResultT


class BlenderMainThreadExecutionStrategy(ExecutionStrategy):
    """Drain worker requests through a timer registered on Blender's main thread."""

    def __init__(self, timers: Any) -> None:
        super().__init__()
        self._timers = timers
        self._pending: Deque[Invocation[Any]] = deque()
        self._timers.register(self._drain, first_interval=0.02, persistent=True)

    def run(self, func: Callable[[], ResultT]) -> ResultT:
        with self._admit():
            invocation = Invocation(func)
            self._pending.append(invocation)
        return invocation.wait()

    def _drain(self) -> Optional[float]:
        with self._lifecycle_lock:
            if self._closed:
                return None
            invocation = self._pending.popleft() if self._pending else None
        if invocation is not None:
            invocation.execute()
        return 0.02

    def _close(self) -> None:
        with self._lifecycle_lock:
            pending = list(self._pending)
            self._pending.clear()
        for invocation in pending:
            invocation.cancel()
