import threading
from typing import Any, Callable, Generic, Optional

from .base import ExecutionStrategyClosedError, ResultT


class Invocation(Generic[ResultT]):
    """A queued call whose result and cancellation are shared across threads.

    Strategies dispatch execute on their host thread and wait on the requesting
    thread. Cancellation releases a waiter only while execution has not started.
    """

    def __init__(self, func: Callable[[], ResultT]) -> None:
        self._func = func
        self._event = threading.Event()
        self._lock = threading.Lock()
        self._result: Any = None
        self._exception: Optional[BaseException] = None
        self._finished = False
        self._started = False

    def execute(self) -> None:
        with self._lock:
            if self._finished or self._started:
                return
            self._started = True
        try:
            self._result = self._func()
        except BaseException as exc:
            self._exception = exc
        finally:
            with self._lock:
                self._finished = True
                self._event.set()

    def cancel(self) -> None:
        with self._lock:
            if self._finished or self._started:
                return
            self._exception = ExecutionStrategyClosedError("execution strategy is closed")
            self._finished = True
            self._event.set()

    def wait(self) -> ResultT:
        self._event.wait()
        if self._exception is not None:
            raise self._exception
        return self._result
