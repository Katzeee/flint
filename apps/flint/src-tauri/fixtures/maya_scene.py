from PySide2 import QtCore, QtWidgets
assert QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
import maya.cmds as cmds
obj = cmds.createNode("transform", name="FlintIntegrationTest")
try:
    cmds.setAttr(obj + ".translateX", 12.5)
    assert cmds.getAttr(obj + ".translateX") == 12.5
finally:
    cmds.delete(obj)
assert not cmds.objExists(obj)
import os, sys
print("SCENE_OK", os.getpid())
print("STDERR_OK", file=sys.stderr)
