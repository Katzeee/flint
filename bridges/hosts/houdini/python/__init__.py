from .. import BridgeManager
from ..execution.capabilities import ExecutionCapabilities
from ..execution.executor import CodeExecutor
from ..execution.scheduling import QtMainThread


def create_execution():
    import hou
    from hutil import Qt

    if not hou.isUIAvailable():
        raise RuntimeError("The Houdini Bridge requires an interactive session")
    return ExecutionCapabilities(CodeExecutor({"hou": hou}), QtMainThread(Qt))


def dispatch_initialization(callback):
    import hou
    import hdefereval

    if not hou.isUIAvailable():
        raise RuntimeError("The Houdini Bridge requires an interactive session")
    hdefereval.executeDeferred(callback)


manager = BridgeManager("houdini", create_execution, dispatch_initialization)
