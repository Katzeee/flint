import threading

import hou

assert threading.current_thread() is threading.main_thread()
node = hou.node("/obj").createNode("geo", "flint_validation")
try:
    box = node.createNode("box")
    assert len(box.geometry().points()) == 8
finally:
    node.destroy()
print("HOUDINI_SCENE_OK")
