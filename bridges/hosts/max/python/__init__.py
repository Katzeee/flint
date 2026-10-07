from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.capabilities import ExecutionCapabilities

from ..execution.scheduling import QtMainThread, dispatch_to_main_thread
from ..qt import resolve_qt


def create_scheduler():
    import pymxs
    pymxs.runtime.maxVersion()
    return QtMainThread(resolve_qt(fallback="PySide2"))


def create_execution():
    return ExecutionCapabilities(CodeExecutor(), create_scheduler())


def dispatch_initialization(callback):
    dispatch_to_main_thread(callback, resolve_qt(fallback="PySide2"))


manager = BridgeManager("max", create_execution, dispatch_initialization)
