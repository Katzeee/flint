import threading

from ..execution.strategies.blender import BlenderMainThreadExecutionStrategy


def create_strategy():
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("Connect the Blender Bridge on Blender's main thread")
    import bpy

    return BlenderMainThreadExecutionStrategy(bpy.app.timers)
