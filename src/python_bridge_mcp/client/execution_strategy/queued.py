import threading
from queue import Empty, Queue
from typing import Any, Callable, Optional

from .base import ExecutionStrategy, ExecutionStrategyClosedError, ResultT


class _Invocation(object):
    def __init__(self, func: Callable[[], Any]) -> None:
        self._func = func
        self._event = threading.Event()
        self._lock = threading.Lock()
        self._result: Any = None
        self._exception: Optional[BaseException] = None
        self._finished = False

    def __call__(self) -> None:
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


class QueuedExecutionStrategy(ExecutionStrategy):
    """Queues callables for an application-owned main-loop pump."""

    def __init__(self, queue: Queue) -> None:
        super().__init__()
        self._queue = queue

    def run(self, func: Callable[[], ResultT]) -> ResultT:
        invocation = _Invocation(func)
        with self._admit():
            self._queue.put(invocation)
        return invocation.wait()

    def _close(self) -> None:
        self._cancel_pending()

    def _cancel_pending(self) -> None:
        retained = []
        while True:
            try:
                item = self._queue.get_nowait()
            except Empty:
                break
            if isinstance(item, _Invocation):
                item.cancel()
            else:
                retained.append(item)
        for item in retained:
            self._queue.put(item)
