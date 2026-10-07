from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.capabilities import ExecutionCapabilities

from ..execution.scheduling import QtMainThread
from ..qt import resolve_qt


def create_scheduler():
    import maya.cmds
    maya.cmds.about(version=True)
    return QtMainThread(resolve_qt(fallback="PySide2"))


def dispatch_initialization(callback):
    """Schedule `callback` on Maya's main thread from any thread."""
    import maya.utils
    maya.utils.executeDeferred(callback)


def create_execution():
    return ExecutionCapabilities(CodeExecutor(), create_scheduler())


manager = BridgeManager("maya", create_execution, dispatch_initialization)
