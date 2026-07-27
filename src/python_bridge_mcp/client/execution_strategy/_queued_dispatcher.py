import threading
from queue import Empty, Queue
from typing import Any, Callable, Optional

from .base import ExecutionStrategyClosedError, ResultT


class _Invocation(object):
    def __init__(self, func: Callable[[], Any]) -> None:
        self._func = func
        self._event = threading.Event()
        self._lock = threading.Lock()
        self._result: Any = None
        self._exception: Optional[BaseException] = None
        self._finished = False

    def execute(self) -> None:
        with self._lock:
            if self._finished:
                return
            try:
                self._result = self._func()
            except BaseException as exc:
                self._exception = exc
            finally:
                self._finished = True
                self._event.set()

    def cancel(self) -> None:
        with self._lock:
            if self._finished:
                return
            self._exception = ExecutionStrategyClosedError("execution strategy is closed")
            self._finished = True
            self._event.set()

    def wait(self) -> Any:
        self._event.wait()
        if self._exception is not None:
            raise self._exception
        return self._result


class QueuedDispatcher(object):
    """Owns queued invocations from submission through execution or cancellation."""

    def __init__(self) -> None:
        self._queue: Queue = Queue()

    def submit(self, func: Callable[[], ResultT]) -> _Invocation:
        invocation = _Invocation(func)
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
