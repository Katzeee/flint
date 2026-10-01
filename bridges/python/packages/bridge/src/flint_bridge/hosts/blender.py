import threading

from ..execution.strategies.blender import BlenderMainThreadExecutionStrategy


def create_strategy():
    if threading.current_thread() is not threading.main_thread():
        raise RuntimeError("Connect the Blender Bridge on Blender's main thread")
    import bpy

    return BlenderMainThreadExecutionStrategy(bpy.app.timers)


def enter_main_thread(callback):
    """Run `callback` once on Blender's main thread from any thread."""
    import bpy

    def once():
        callback()
        return None

    bpy.app.timers.register(once, first_interval=0.0)
