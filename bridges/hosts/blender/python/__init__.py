from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.capabilities import ExecutionCapabilities

import threading

from .scheduler import BlenderTimerQueue


def create_scheduler():
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("Connect the Blender Bridge on Blender's main thread")
    import bpy

    return BlenderTimerQueue(bpy.app.timers)


def dispatch_initialization(callback):
    """Run `callback` once on Blender's main thread from any thread."""
    import bpy

    def once():
        callback()
        return None

    bpy.app.timers.register(once, first_interval=0.0)


def create_execution():
    return ExecutionCapabilities(CodeExecutor(), create_scheduler())


manager = BridgeManager("blender", create_execution, dispatch_initialization)
