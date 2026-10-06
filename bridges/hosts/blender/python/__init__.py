from .. import BridgeManager
from ..execution.executor import CodeExecutor
from ..execution.runner import CodeRunner

import threading

from .strategy import BlenderMainThreadExecutionStrategy


def create_strategy():
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("Connect the Blender Bridge on Blender's main thread")
    import bpy

    return BlenderMainThreadExecutionStrategy(bpy.app.timers)


def dispatch_initialization(callback):
    """Run `callback` once on Blender's main thread from any thread."""
    import bpy

    def once():
        callback()
        return None

    bpy.app.timers.register(once, first_interval=0.0)


def create_runner():
    return CodeRunner(CodeExecutor(), create_strategy())


manager = BridgeManager("blender", create_runner, dispatch_initialization)
