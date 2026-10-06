from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.runner import CodeRunner

from ..execution.strategies import QtMainThreadExecutionStrategy
from ..execution.strategies.qt import dispatch_to_main_thread
from ..qt import resolve_qt


def create_strategy():
    strategy = QtMainThreadExecutionStrategy(resolve_qt(fallback="PySide2"))
    try:
        import pymxs
        pymxs.runtime.maxVersion()
        return strategy
    except BaseException:
        strategy.close()
        raise


def create_runner():
    return CodeRunner(CodeExecutor(), create_strategy())


def dispatch_initialization(callback):
    dispatch_to_main_thread(callback, resolve_qt(fallback="PySide2"))


manager = BridgeManager("max", create_runner, dispatch_initialization)
