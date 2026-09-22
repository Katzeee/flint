from ..execution.strategies import QtMainThreadExecutionStrategy


def create_strategy():
    try:
        from PySide6 import QtCore, QtWidgets
    except ImportError:
        from PySide2 import QtCore, QtWidgets
    from types import SimpleNamespace
    app = QtWidgets.QApplication.instance()
    if app is None or QtCore.QThread.currentThread() is not app.thread():
        raise RuntimeError("Connect flint from the host UI thread")
    return QtMainThreadExecutionStrategy(qt=SimpleNamespace(QtCore=QtCore, QtWidgets=QtWidgets))
