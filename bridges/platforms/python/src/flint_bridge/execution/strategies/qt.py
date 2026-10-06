from typing import Any, Callable, Optional

from ...qt import resolve_qt
from .invocation import Invocation
from .base import ExecutionStrategy, ResultT


def _create_dispatcher(qt):
    QtCore, QtWidgets = qt.QtCore, qt.QtWidgets
    app = QtWidgets.QApplication.instance()
    if app is None:
        raise RuntimeError("Host has no running Qt application")

    class Dispatcher(QtCore.QObject):
        requested = QtCore.Signal(object)

        def __init__(self):
            super().__init__()
            self.moveToThread(app.thread())
            self.requested.connect(self.execute)

        def dispatch(self, callback):
            self.requested.emit(callback)

        @QtCore.Slot(object)
        def execute(self, callback):
            callback()

    return Dispatcher()


class QtMainThreadExecutionStrategy(ExecutionStrategy):
    def __init__(self, qt: Optional[Any] = None) -> None:
        super().__init__()
        qt = qt if qt is not None else resolve_qt()
        app = qt.QtWidgets.QApplication.instance()
        if app is None or qt.QtCore.QThread.currentThread() is not app.thread():
            raise RuntimeError("Create the Qt execution strategy on the application's main thread")
        self._dispatcher = _create_dispatcher(qt)
        self._pending = set()

    def run(self, func: Callable[[], ResultT]) -> ResultT:
        with self._admit():
            invocation = Invocation(func)
            self._pending.add(invocation)
        try:
            self._dispatcher.dispatch(invocation.execute)
            return invocation.wait()
        finally:
            with self._lifecycle_lock:
                self._pending.discard(invocation)

    def _close(self) -> None:
        with self._lifecycle_lock:
            pending = list(self._pending)
        for invocation in pending:
            invocation.cancel()


# Keep queued initialization callbacks alive until the UI thread receives them.
_entries = set()


def dispatch_to_main_thread(callback, qt=None):
    dispatcher = _create_dispatcher(qt if qt is not None else resolve_qt())

    def run():
        try:
            callback()
        finally:
            _entries.discard(dispatcher)

    _entries.add(dispatcher)
    try:
        dispatcher.dispatch(run)
    except BaseException:
        _entries.discard(dispatcher)
        raise
