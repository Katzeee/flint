import os
import sys
from PySide2 import QtCore, QtWidgets

assert QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
import maya.cmds as cmds  # noqa: E402 - verify the host execution thread before importing its API

obj = cmds.createNode("transform", name="FlintIntegrationTest")
try:
    cmds.setAttr(obj + ".translateX", 12.5)
    assert cmds.getAttr(obj + ".translateX") == 12.5
finally:
    cmds.delete(obj)
assert not cmds.objExists(obj)
print("SCENE_OK", os.getpid())
print("STDERR_OK", file=sys.stderr)
