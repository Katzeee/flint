"""Maya user settings and connection controls for the Flint plug-in."""

import maya.cmds as cmds
import maya.utils
from flint_bridge import BridgeCreationError
from flint_bridge.maya import manager

_KEYS = {
    "address": ("flint_bridge_address", "127.0.0.1"),
    "port": ("flint_bridge_port", 6321),
    "name": ("flint_instance_name", "Maya"),
    "enabled": ("flint_connection_enabled", 1),
}
_MENU = "FlintBridgeMenu"
_loaded = False
_dialog = None


def settings():
    values = {}
    for field, (key, default) in _KEYS.items():
        values[field] = cmds.optionVar(query=key) if cmds.optionVar(exists=key) else default
    values["port"] = int(values["port"])
    values["enabled"] = bool(values["enabled"])
    return values


def apply_settings(address, port, name, enabled):
    """Apply a complete endpoint configuration, then persist it for Maya."""
    port = int(port)
    enabled = bool(enabled)
    bridge = manager.configure(address=address, port=port, name=name, enabled=enabled)
    cmds.optionVar(stringValue=(_KEYS["address"][0], address))
    cmds.optionVar(intValue=(_KEYS["port"][0], port))
    cmds.optionVar(stringValue=(_KEYS["name"][0], name))
    cmds.optionVar(intValue=(_KEYS["enabled"][0], int(enabled)))
    return bridge


def reconnect():
    return manager.reconnect()


def show_settings(*_):
    from flint_bridge.qt import resolve_qt

    qt = resolve_qt(fallback="PySide2")
    QtWidgets = qt.QtWidgets
    from maya import OpenMayaUI

    if qt.binding == "PySide6":
        from shiboken6 import wrapInstance
    else:
        from shiboken2 import wrapInstance
    from flint_bridge.ui.connection_panel import ConnectionPanel

    global _dialog
    if _dialog is not None:
        _dialog.close()
    parent = wrapInstance(int(OpenMayaUI.MQtUtil.mainWindow()), QtWidgets.QWidget)
    _dialog = ConnectionPanel(
        settings(), lambda: manager.current().status if manager.current() else None, apply_settings, reconnect, parent
    )
    _dialog.show()


def _install_menu():
    if not _loaded or cmds.about(batch=True) or cmds.menu(_MENU, exists=True):
        return
    cmds.menu(_MENU, label="Flint", parent="MayaWindow", tearOff=True)
    cmds.menuItem(label="Connection Settings", parent=_MENU, command=show_settings)


def initialize():
    global _loaded
    values = settings()
    try:
        manager.connect(address=values["address"], port=values["port"], name=values["name"], enabled=values["enabled"])
    except BridgeCreationError as error:
        # The settings panel stays available so the user can start it with Apply.
        cmds.warning("Flint Bridge did not start: {}".format(error))
    _loaded = True
    maya.utils.executeDeferred(_install_menu)


def uninitialize():
    global _loaded, _dialog
    if not manager.disconnect():
        raise RuntimeError("Flint Bridge is still executing host code")
    _loaded = False
    if _dialog is not None:
        _dialog.close()
        _dialog = None
    if cmds.menu(_MENU, exists=True):
        cmds.deleteUI(_MENU)
