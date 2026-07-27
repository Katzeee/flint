from queue import Empty, Queue
from typing import Callable

from ._invocation import Invocation
from .base import ResultT


class QueuedDispatcher(object):
    """Owns queued invocations from submission through execution or cancellation."""

    def __init__(self) -> None:
        self._queue: Queue = Queue()

    def submit(self, func: Callable[[], ResultT]) -> Invocation[ResultT]:
        invocation = Invocation(func)
        self._queue.put(invocation)
        return invocation

    def execute_pending(self, limit: int) -> int:
        executed = 0
        while executed < limit:
            try:
                invocation = self._queue.get_nowait()
            except Empty:
                break
            invocation.execute()
            executed += 1
        return executed

    def cancel_pending(self) -> int:
        cancelled = 0
        while True:
            try:
                invocation = self._queue.get_nowait()
            except Empty:
                break
            invocation.cancel()
            cancelled += 1
        return cancelled
