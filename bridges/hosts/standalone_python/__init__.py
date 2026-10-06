from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.runner import CodeRunner

from ..execution.strategies import DirectExecutionStrategy


def create_strategy():
    return DirectExecutionStrategy()


def dispatch_initialization(callback):
    callback()


def create_runner():
    return CodeRunner(CodeExecutor(), create_strategy())


manager = BridgeManager("standalone_python", create_runner, dispatch_initialization)
