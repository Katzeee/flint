import threading
assert threading.current_thread() is threading.main_thread()
import bpy
bpy.ops.mesh.primitive_cube_add()
obj = bpy.context.object
try:
    obj.name = "FlintIntegrationTest"
    obj.location.x = 12.5
    assert obj.location.x == 12.5
finally:
    bpy.data.objects.remove(obj, do_unlink=True)
assert bpy.data.objects.get("FlintIntegrationTest") is None
import os, sys
print("SCENE_OK", os.getpid())
print("STDERR_OK", file=sys.stderr)
