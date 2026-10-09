import os
import sys
import threading

assert threading.current_thread() is threading.main_thread()
import bpy  # noqa: E402 - verify the host execution thread before importing its API

bpy.ops.mesh.primitive_cube_add()
obj = bpy.context.object
try:
    obj.name = "FlintIntegrationTest"
    obj.location.x = 12.5
    assert obj.location.x == 12.5
finally:
    bpy.data.objects.remove(obj, do_unlink=True)
assert bpy.data.objects.get("FlintIntegrationTest") is None
print("SCENE_OK", os.getpid())
print("STDERR_OK", file=sys.stderr)
