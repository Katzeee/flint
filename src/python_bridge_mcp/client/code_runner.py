import asyncio
import functools
import threading
from abc import ABC, abstractmethod
from queue import Queue
from typing import Any, Callable, Optional, Type, TypeVar

from ..shared.instance_control_models import InstanceExecResult
from ..shared.text_buffer import ThreadSafeTextBuffer
from .code_executor import CodeExecutor

RunResultT = TypeVar("RunResultT")


class CodeRunner(ABC):
    """Base class for code execution strategies."""

    def __init__(self, executor: CodeExecutor) -> None:
        self._executor = executor

    @classmethod
    def create_default(cls, executor: Optional[CodeExecutor] = None) -> "CodeRunner":
        executor = executor if executor is not None else CodeExecutor()
        try:
            return QtMainThreadRunner(executor)
        except ImportError:
            return DirectRunner(executor)

    @abstractmethod
    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult: ...

    async def async_execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(
            None, functools.partial(self.execute, execution_id, code, out=out, err=err, filename=filename),
        )


class DirectRunner(CodeRunner):
    """Runs code immediately in the calling thread."""

    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        return self._executor.execute(execution_id, code, out=out, err=err, filename=filename)


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
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        result_event = threading.Event()
        holder: list = [None]

        def _task() -> None:
            holder[0] = self._executor.execute(execution_id, code, out=out, err=err, filename=filename)
            result_event.set()

        self._queue.put(_task)
        result_event.wait()
        return holder[0]


class _QtExecutor(object):
    def __init__(self, qt: Any):
        QtCore = qt.QtCore
        QtWidgets = qt.QtWidgets

        class _Payload(object):
            def __init__(self, func: Callable[[], Any], signal: Any):
                self.func = func
                self.result: Any = None
                self.exception: Optional[Exception] = None
                self.signal = signal
                self.wcnd = QtCore.QWaitCondition()
                self.mutex = QtCore.QMutex()

            def wait(self) -> Any:
                self.mutex.lock()
                self.signal.emit(self)
                self.wcnd.wait(self.mutex)
                self.mutex.unlock()
                if self.exception:
                    raise self.exception
                return self.result

            def run(self) -> None:
                self.mutex.lock()
                try:
                    self.result = self.func()
                except Exception as exc:
                    self.exception = exc
                self.wcnd.wakeAll()
                self.mutex.unlock()

        class _Bridge(QtCore.QObject):
            sig: Any = QtCore.Signal(object)

            def __init__(self) -> None:
                super().__init__()  # pyright: ignore[reportUnknownMemberType]
                app = QtWidgets.QApplication.instance()
                if app is not None:
                    self.moveToThread(app.thread())
                self.sig.connect(self._on_signal)

            def _on_signal(self, payload: Any) -> None:
                payload.run()

        self._bridge: _Bridge = _Bridge()
        self._Payload: Type[_Payload] = _Payload
        self._QtCore = QtCore
        self._QtWidgets = QtWidgets

    def run(self, func: Callable[[], RunResultT]) -> RunResultT:
        app = self._QtWidgets.QApplication.instance()
        if app and self._QtCore.QThread.currentThread() is app.thread():
            return func()
        payload = self._Payload(func, self._bridge.sig)
        return payload.wait()


class QtMainThreadRunner(CodeRunner):
    def __init__(self, executor: CodeExecutor) -> None:
        super().__init__(executor)
        self._executor_bridge = _QtExecutor(self._load_qt())

    def execute(
        self,
        execution_id: str,
        code: str,
        out: Optional[ThreadSafeTextBuffer] = None,
        err: Optional[ThreadSafeTextBuffer] = None,
        filename: Optional[str] = None,
    ) -> InstanceExecResult:
        return self._executor_bridge.run(
            lambda: self._executor.execute(execution_id, code, out=out, err=err, filename=filename)
        )

    @staticmethod
    def _load_qt() -> Any:
        try:
            import PySide6  # pyright: ignore[reportMissingImports]

            return PySide6
        except ImportError:
            pass
        try:
            import PySide2  # pyright: ignore[reportMissingImports]

            return PySide2
        except ImportError:
            pass
        raise ImportError("Neither PySide6 nor PySide2 is available")
