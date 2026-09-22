import threading
from abc import ABC, abstractmethod
from contextlib import contextmanager
from typing import Callable, Iterator, TypeVar


ResultT = TypeVar("ResultT")


class ExecutionStrategyClosedError(RuntimeError):
    pass


class ExecutionStrategy(ABC):
    """Chooses the host thread on which a callable is executed."""

    def __init__(self) -> None:
        self._lifecycle_lock = threading.Lock()
        self._closed = False

    @abstractmethod
    def run(self, func: Callable[[], ResultT]) -> ResultT: ...

    @contextmanager
    def _admit(self) -> Iterator[None]:
        """Atomically admit work while the strategy is open."""
        with self._lifecycle_lock:
            if self._closed:
                raise ExecutionStrategyClosedError("execution strategy is closed")
            yield

    def close(self) -> None:
        """Permanently reject new work and release host-specific resources."""
        with self._lifecycle_lock:
            if self._closed:
                return
            self._closed = True
        self._close()

    def _close(self) -> None:
        """Release resources after the terminal state transition."""
