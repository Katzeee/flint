"""Runs inside a fresh DCC process. Rust owns the test lifecycle."""

import json
import os
from pathlib import Path
import sys
import traceback

# Rust supplies this value before executing the fixture in the host.
CONFIG = globals()["CONFIG"]

report = {"pid": os.getpid(), "python": sys.version, "host": CONFIG["host"]}
if CONFIG["host"] != "blender":
    from PySide2 import QtCore, QtWidgets


def capture_settings_window():
    QtWidgets.QApplication.processEvents()
    dialogs = [
        widget
        for widget in QtWidgets.QApplication.topLevelWidgets()
        if widget.windowTitle() == "Flint Bridge" and widget.isVisible()
    ]
    if not dialogs:
        raise RuntimeError("The Flint Bridge settings window is not visible")
    if not dialogs[-1].grab().save(CONFIG["screenshot"]):
        raise RuntimeError("Host settings screenshot failed")
    return True


try:
    if CONFIG["host"] == "blender":
        import bpy
        import threading

        report["main_thread"] = threading.current_thread() is threading.main_thread()
        bpy.ops.preferences.addon_install(filepath=CONFIG["bundle"], overwrite=True, enable_on_install=False)
        import flint_blender

        # Populate the isolated profile before the add-on's first connection.
        addon = bpy.context.preferences.addons.new()
        addon.module = "flint_blender"
        bpy.utils.register_class(flint_blender.FlintBridgePreferences)
        addon.preferences.address = "127.0.0.1"
        addon.preferences.port = CONFIG["port"]
        addon.preferences.instance_name = "Blender"
        bpy.utils.unregister_class(flint_blender.FlintBridgePreferences)
        bpy.ops.preferences.addon_enable(module="flint_blender")
        report["addon_enabled"] = "flint_blender" in bpy.context.preferences.addons
        if not report["addon_enabled"]:
            raise RuntimeError("The Blender Add-on was not enabled")
        report["package_module"] = flint_blender.__file__
        from flint_blender.flint_bridge.blender import manager
    else:
        report["main_thread"] = QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
    if not report["main_thread"]:
        raise RuntimeError("The host must bootstrap on its UI thread")
    if CONFIG["host"] == "maya":
        import maya.cmds as cmds

        cmds.optionVar(intValue=("flint_bridge_port", CONFIG["port"]))
        cmds.loadPlugin("flint_plugin.py", quiet=True)
        report["plugin_loaded"] = cmds.pluginInfo("flint_plugin.py", query=True, loaded=True)
        import flint_maya

        report["package_module"] = flint_maya.__file__
        flint_maya.show_settings()
        report["settings_visible"] = capture_settings_window()
        from flint_bridge.maya import manager

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

        report["package_module"] = flint_max.__file__
        flint_max.show_settings()
        report["settings_visible"] = capture_settings_window()
        from flint_bridge.max import manager
    else:
        report["version"] = bpy.app.version_string
        report["scene"] = bpy.data.filepath
    bridge = manager.current()
    if bridge is None:
        raise RuntimeError("The host package did not start a Bridge")
    if not bridge.wait_until_connected(15):
        raise RuntimeError("Both bridge channels did not connect")
    if CONFIG["host"] == "blender":
        if bpy.ops.flint_bridge.disconnect() != {"FINISHED"}:
            raise RuntimeError("The Blender Add-on did not disconnect")
        for _ in range(3):
            flint_blender._refresh_ui()
            if manager.current() is not None:
                raise RuntimeError("UI refresh restarted the disconnected Bridge")
        if bpy.ops.flint_bridge.connect() != {"FINISHED"}:
            raise RuntimeError("The Blender Add-on did not reconnect")
    else:
        panel = flint_maya._dialog if CONFIG["host"] == "maya" else flint_max._dialog
        panel.connection_button.click()
        for _ in range(3):
            QtWidgets.QApplication.processEvents()
            panel.refresh()
            if manager.current() is not None:
                raise RuntimeError("UI refresh restarted the disconnected Bridge")
        panel.connection_button.click()
    bridge = manager.current()
    if bridge is None or not bridge.wait_until_connected(15):
        raise RuntimeError("Explicit Connect did not restore the connection")
    report["disconnect_and_connect"] = True
    report["instance_id"] = bridge.instance_id
except BaseException:
    report["error"] = traceback.format_exc()
Path(CONFIG["ready"]).write_text(json.dumps(report), encoding="utf-8")
