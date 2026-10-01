import sys
from types import SimpleNamespace

from ..execution.strategies import QtMainThreadExecutionStrategy


def create_strategy():
    # A host may have more than one binding installed. Reuse the binding that
    # owns its running QApplication before importing another Qt runtime.
    for binding in ("PySide2", "PySide6"):
        core = sys.modules.get(binding + ".QtCore")
        widgets = sys.modules.get(binding + ".QtWidgets")
        if core is None or widgets is None:
            continue
        app = widgets.QApplication.instance()
        if app is not None and core.QThread.currentThread() is app.thread():
            return QtMainThreadExecutionStrategy(qt=SimpleNamespace(QtCore=core, QtWidgets=widgets))
    # Maya 2024 and Max 2024 can expose Qt before their Python binding has
    # been imported by a user's startup script.
    try:
        from PySide2 import QtCore, QtWidgets
    except ImportError:
        raise RuntimeError("No supported Qt binding is available in this host")
    app = QtWidgets.QApplication.instance()
    if app is not None and QtCore.QThread.currentThread() is app.thread():
        return QtMainThreadExecutionStrategy(qt=SimpleNamespace(QtCore=QtCore, QtWidgets=QtWidgets))
    raise RuntimeError("Connect flint from the host UI thread after its Qt binding is initialized")


def _active_qt():
    """Return the Qt binding that owns the host's running QApplication."""
    for binding in ("PySide2", "PySide6"):
        core = sys.modules.get(binding + ".QtCore")
        widgets = sys.modules.get(binding + ".QtWidgets")
        if core is not None and widgets is not None and widgets.QApplication.instance() is not None:
            return SimpleNamespace(QtCore=core, QtWidgets=widgets)
    try:
        from PySide2 import QtCore, QtWidgets
    except ImportError:
        raise RuntimeError("No supported Qt binding is available in this host")
    return SimpleNamespace(QtCore=QtCore, QtWidgets=QtWidgets)


# References to in-flight entries, released once the host thread has run them.
_entries = set()


def enter_main_thread(callback):
    """Schedule `callback` on the host's Qt main thread from any thread."""
    qt = _active_qt()
    QtCore, QtWidgets = qt.QtCore, qt.QtWidgets
    app = QtWidgets.QApplication.instance()
    if app is None:
        raise RuntimeError("Host has no QApplication")

    class _Entry(QtCore.QObject):
        requested = QtCore.Signal()

        def __init__(self):
            super().__init__()
            self.moveToThread(app.thread())
            self.requested.connect(self._run, QtCore.Qt.QueuedConnection)

        @QtCore.Slot()
        def _run(self):
            try:
                callback()
            finally:
                _entries.discard(self)

    entry = _Entry()
    _entries.add(entry)
    entry.requested.emit()
