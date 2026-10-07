from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.capabilities import ExecutionCapabilities

from ..execution.scheduling import WorkerThread


def dispatch_initialization(callback):
    callback()


def create_execution():
    return ExecutionCapabilities(CodeExecutor(), WorkerThread())


manager = BridgeManager("standalone_python", create_execution, dispatch_initialization)
