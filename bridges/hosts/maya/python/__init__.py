from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.runner import CodeRunner

from ..execution.strategies import QtMainThreadExecutionStrategy
from ..qt import resolve_qt


def create_strategy():
    strategy = QtMainThreadExecutionStrategy(resolve_qt(fallback="PySide2"))
    try:
        import maya.cmds
        maya.cmds.about(version=True)
        return strategy
    except BaseException:
        strategy.close()
        raise


def dispatch_initialization(callback):
    """Schedule `callback` on Maya's main thread from any thread."""
    import maya.utils
    maya.utils.executeDeferred(callback)


def create_runner():
    return CodeRunner(CodeExecutor(), create_strategy())


manager = BridgeManager("maya", create_runner, dispatch_initialization)
