from PySide2 import QtCore, QtWidgets
assert QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
import pymxs
rt = pymxs.runtime
obj = rt.Point(name="FlintIntegrationTest")
try:
    obj.pos = rt.Point3(12.5, 0, 0)
    assert obj.pos.x == 12.5
finally:
    rt.delete(obj)
assert rt.getNodeByName("FlintIntegrationTest") is None
import os, sys
print("SCENE_OK", os.getpid())
print("STDERR_OK", file=sys.stderr)
