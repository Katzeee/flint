"""Schedulers that run Bridge callbacks on a host's execution thread."""
from collections import deque
import queue
import threading

from ..qt import resolve_qt


class WorkerThread:
    """A dedicated execution thread for hosts without an application loop."""

    def __init__(self):
        self._callbacks = queue.Queue()
        threading.Thread(target=self._serve, name="flint-execution", daemon=True).start()

    def post(self, callback):
        self._callbacks.put(callback)

    def close(self):
        self._callbacks.put(None)

    def _serve(self):
        for callback in iter(self._callbacks.get, None):
            callback()


class CallbackQueue:
    """Holds callbacks until the host drains them from its own loop."""

    def __init__(self):
        self._callbacks = deque()

    def post(self, callback):
        self._callbacks.append(callback)

    def drain(self):
        for _ in range(len(self._callbacks)):
            self._callbacks.popleft()()

    def close(self):
        self.drain()


class QtMainThread:
    """Posts callbacks to the running Qt application's thread from any thread."""

    def __init__(self, qt=None):
        qt = qt if qt is not None else resolve_qt()
        QtCore = qt.QtCore
        app = qt.QtWidgets.QApplication.instance()
        if app is None:
            raise RuntimeError("Host has no running Qt application")

        class Receiver(QtCore.QObject):
            requested = QtCore.Signal(object)

            @QtCore.Slot(object)
            def execute(self, callback):
                callback()

        self._receiver = Receiver()
        self._receiver.moveToThread(app.thread())
        self._receiver.requested.connect(self._receiver.execute, QtCore.Qt.QueuedConnection)

    def post(self, callback):
        self._receiver.requested.emit(callback)

    def close(self):
        pass


# Keep one-shot schedulers alive until the application thread receives their callback.
_pending = set()


def dispatch_to_main_thread(callback, qt=None):
    scheduler = QtMainThread(qt)
    _pending.add(scheduler)

    def run():
        _pending.discard(scheduler)
        callback()

    scheduler.post(run)
