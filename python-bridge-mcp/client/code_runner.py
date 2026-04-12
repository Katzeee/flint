import asyncio
import functools
import threading
from abc import ABC, abstractmethod
from queue import Queue
from typing import Optional

from ..shared.exec_models import ExecResult
from ..shared.text_buffer import ThreadSafeTextBuffer
from .code_executor import CodeExecutor


class CodeRunner(ABC):
    """Base class for code execution strategies."""

    def __init__(self, executor: CodeExecutor) -> None:
        self._executor = executor

    @abstractmethod
    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
    ) -> ExecResult: ...

    async def async_execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
    ) -> ExecResult:
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(
            None, functools.partial(self.execute, execution_id, code, out=out, err=err),
        )


class DirectRunner(CodeRunner):
    """Runs code immediately in the calling thread."""

    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
    ) -> ExecResult:
        return self._executor.execute(execution_id, code, out=out, err=err)


class MainThreadRunner(CodeRunner):
    """Sends code to the main thread via a queue and blocks until done.

    The caller must drain the queue in the main thread loop, e.g.::

        while True:
            task = queue.get()
            task()
    """

    def __init__(self, executor: CodeExecutor, queue: Queue) -> None:
        super().__init__(executor)
        self._queue = queue

    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
    ) -> ExecResult:
        result_event = threading.Event()
        holder: list = [None]

        def _task() -> None:
            holder[0] = self._executor.execute(execution_id, code, out=out, err=err)
            result_event.set()

        self._queue.put(_task)
        result_event.wait()
        return holder[0]
