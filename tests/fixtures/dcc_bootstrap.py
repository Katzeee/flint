"""Runs inside a fresh DCC process. Rust owns the test lifecycle."""
import json
import os
from pathlib import Path
import sys
import traceback

report = {"pid": os.getpid(), "python": sys.version, "host": CONFIG["host"]}
try:
    if CONFIG["host"] == "blender":
        import bpy
        import threading
        report["main_thread"] = threading.current_thread() is threading.main_thread()
        bpy.ops.preferences.addon_install(filepath=CONFIG["bundle"], overwrite=True,
                                          enable_on_install=True)
        report["addon_enabled"] = "flint_blender" in bpy.context.preferences.addons
        if not report["addon_enabled"]:
            raise RuntimeError("The Blender Add-on was not enabled")
        bridge = getattr(sys, "_flint_bridge_service", None)
        if bridge is None:
            raise RuntimeError("The Blender Add-on did not start a Bridge")
    else:
        sys.path.insert(0, CONFIG["bundle"])
        from flint_bridge import connect
        from PySide2 import QtCore, QtWidgets
        report["main_thread"] = QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
    if not report["main_thread"]:
        raise RuntimeError("The host must bootstrap on its UI thread")
    if CONFIG["host"] == "maya":
        import maya.cmds as cmds
        report["version"] = cmds.about(version=True)
        report["scene"] = cmds.file(query=True, sceneName=True)
    elif CONFIG["host"] == "max":
        import pymxs
        report["version"] = str(pymxs.runtime.maxVersion())
        report["scene"] = str(pymxs.runtime.maxFileName)
    else:
        report["version"] = bpy.app.version_string
        report["scene"] = bpy.data.filepath
    if CONFIG["host"] != "blender":
        bridge = connect(host=CONFIG["host"], name="Rust integration test", port=CONFIG["port"])
    if not bridge.wait_until_connected(15):
        raise RuntimeError("Both bridge channels did not connect")
    report["instance_id"] = bridge.instance_id
except BaseException:
    report["error"] = traceback.format_exc()
Path(CONFIG["ready"]).write_text(json.dumps(report), encoding="utf-8")
