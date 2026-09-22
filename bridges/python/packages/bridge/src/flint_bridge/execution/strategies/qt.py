from typing import Any, Callable, Optional

from ._invocation import Invocation
from .base import ExecutionStrategy, ResultT


class QtMainThreadExecutionStrategy(ExecutionStrategy):
    def __init__(self, qt: Optional[Any] = None) -> None:
        super().__init__()
        qt = qt if qt is not None else self._load_qt()
        self._bridge = self._create_signal_bridge(qt.QtCore, qt.QtWidgets)
        self._pending = set()

    def run(self, func: Callable[[], ResultT]) -> ResultT:
        with self._admit():
            invocation = Invocation(func)
            self._pending.add(invocation)
        try:
            self._bridge.dispatch(invocation)
            return invocation.wait()
        finally:
            with self._lifecycle_lock:
                self._pending.discard(invocation)

    def _close(self) -> None:
        with self._lifecycle_lock:
            pending = list(self._pending)
        for invocation in pending:
            invocation.cancel()

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
    def _create_signal_bridge(QtCore: Any, QtWidgets: Any) -> Any:
        class _QtSignalBridge(QtCore.QObject):
            invocation_requested: Any = QtCore.Signal(object)

            def __init__(self) -> None:
                super().__init__()  # pyright: ignore[reportUnknownMemberType]
                app = QtWidgets.QApplication.instance()
                if app is not None:
                    self.moveToThread(app.thread())
                self.invocation_requested.connect(self._execute)

            def dispatch(self, invocation: Invocation[Any]) -> None:
                self.invocation_requested.emit(invocation)

            def _execute(self, invocation: Invocation[Any]) -> None:
                invocation.execute()

        return _QtSignalBridge()
