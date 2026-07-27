from typing import Any, Callable, Optional

from .base import ExecutionStrategy, ResultT


class QtMainThreadExecutionStrategy(ExecutionStrategy):
    def __init__(self, qt: Optional[Any] = None) -> None:
        super().__init__()
        self._qt = qt if qt is not None else self._load_qt()
        self._executor = self._create_executor(self._qt)

    def run(self, func: Callable[[], ResultT]) -> ResultT:
        with self._admit():
            pass
        return self._executor.run(func)

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

    @staticmethod
    def _create_executor(qt: Any) -> Any:
        QtCore = qt.QtCore
        QtWidgets = qt.QtWidgets

        class _Payload(object):
            def __init__(self, func: Callable[[], Any], signal: Any):
                self.func = func
                self.result: Any = None
                self.exception: Optional[BaseException] = None
                self.signal = signal
                self.wcnd = QtCore.QWaitCondition()
                self.mutex = QtCore.QMutex()

            def wait(self) -> Any:
                self.mutex.lock()
                self.signal.emit(self)
                self.wcnd.wait(self.mutex)
                self.mutex.unlock()
                if self.exception is not None:
                    raise self.exception
                return self.result

            def run(self) -> None:
                self.mutex.lock()
                try:
                    self.result = self.func()
                except BaseException as exc:
                    self.exception = exc
                finally:
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

        class _Executor(object):
            def __init__(self) -> None:
                self._bridge = _Bridge()

            def run(self, func: Callable[[], ResultT]) -> ResultT:
                app = QtWidgets.QApplication.instance()
                if app and QtCore.QThread.currentThread() is app.thread():
                    return func()
                payload = _Payload(func, self._bridge.sig)
                return payload.wait()

        return _Executor()
