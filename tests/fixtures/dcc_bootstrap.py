"""Runs inside a fresh DCC process. Rust owns the test lifecycle."""
import json
import os
from pathlib import Path
import sys
import traceback

report = {"pid": os.getpid(), "python": sys.version, "host": CONFIG["host"]}
if CONFIG["host"] != "blender":
    from PySide2 import QtCore, QtWidgets

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
        draft = bpy.context.window_manager.flint_bridge_draft
        draft.port = CONFIG["port"]
        if bpy.ops.flint_bridge.apply_settings() != {"FINISHED"}:
            raise RuntimeError("The Blender Add-on did not apply its settings")
        bridge = getattr(sys, "_flint_bridge_service", None)
        if bridge is None:
            raise RuntimeError("The Blender Add-on did not start a Bridge")
    else:
        report["main_thread"] = QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
    if not report["main_thread"]:
        raise RuntimeError("The host must bootstrap on its UI thread")
    if CONFIG["host"] == "maya":
        import maya.cmds as cmds
        cmds.optionVar(intValue=("flint_registry_port", CONFIG["port"]))
        cmds.loadPlugin("flint_plugin.py", quiet=True)
        report["plugin_loaded"] = cmds.pluginInfo("flint_plugin.py", query=True, loaded=True)
        import flint_maya
        flint_maya.show_settings()
        report["settings_visible"] = flint_maya._dialog.isVisible()
        QtWidgets.QApplication.processEvents()
        dialogs = [widget for widget in QtWidgets.QApplication.topLevelWidgets()
                   if widget.windowTitle() == "Flint Bridge" and widget.isVisible()]
        if not dialogs or not dialogs[-1].grab().save(CONFIG["screenshot"]):
            raise RuntimeError("Maya settings screenshot failed")
        report["version"] = cmds.about(version=True)
        report["scene"] = cmds.file(query=True, sceneName=True)
    elif CONFIG["host"] == "max":
        import pymxs
        report["version"] = str(pymxs.runtime.maxVersion())
        report["scene"] = str(pymxs.runtime.maxFileName)
        packages = pymxs.runtime.PluginPackageManager
        report["startup_script_registered"] = any(
            "flint_startup.ms" in str(packages.GetPostStartUpScriptFullPath(index))
            for index in range(1, packages.GetPostStartUpScriptsCount() + 1)
        )
        import flint_max
        flint_max.show_settings()
        report["settings_visible"] = flint_max._dialog.isVisible()
        QtWidgets.QApplication.processEvents()
        if not flint_max._dialog.grab().save(CONFIG["screenshot"]):
            raise RuntimeError("3ds Max settings screenshot failed")
    else:
        report["version"] = bpy.app.version_string
        report["scene"] = bpy.data.filepath
    if CONFIG["host"] != "blender":
        bridge = getattr(sys, "_flint_bridge_service", None)
        if bridge is None:
            raise RuntimeError("The host package did not start a Bridge")
    if not bridge.wait_until_connected(15):
        raise RuntimeError("Both bridge channels did not connect")
    report["instance_id"] = bridge.instance_id
except BaseException:
    report["error"] = traceback.format_exc()
Path(CONFIG["ready"]).write_text(json.dumps(report), encoding="utf-8")
