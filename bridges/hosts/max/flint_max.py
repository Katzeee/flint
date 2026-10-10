"""3ds Max user settings and connection controls for Flint Bridge."""

from pymxs import runtime as rt
from pathlib import Path
from flint_bridge import BridgeCreationError
from flint_bridge.max import manager

_SECTION = "Flint Bridge"
_DEFAULTS = {"address": "127.0.0.1", "port": 6321, "name": "3ds Max"}
_KEYS = {"address": "BridgeAddress", "port": "BridgePort", "name": "InstanceName"}
_dialog = None


def _ini():
    return str(Path(str(rt.getDir(rt.Name("maxData")))) / "FlintBridge.ini")


def settings():
    values = {}
    for field, default in _DEFAULTS.items():
        value = str(rt.getINISetting(_ini(), _SECTION, _KEYS[field]))
        values[field] = value if value else default
    values["port"] = int(values["port"])
    return values


def apply_settings(address, port, name):
    port = int(port)
    for field, value in {"address": address, "port": port, "name": name}.items():
        rt.setINISetting(_ini(), _SECTION, _KEYS[field], str(value))


def reconnect():
    return manager.reconnect()


def initialize():
    try:
        manager.connect(**settings())
    except BridgeCreationError as error:
        # The settings panel stays available so the user can start it with Connect.
        print("Flint Bridge did not start: {}".format(error))


def show_settings():
    from qtmax import GetQMaxMainWindow
    from flint_bridge.qt import resolve_qt

    resolve_qt(fallback="PySide2")
    from flint_bridge.ui.connection_panel import ConnectionPanel

    global _dialog
    if _dialog is not None:
        _dialog.close()
    _dialog = ConnectionPanel(
        settings,
        lambda: manager.current().status if manager.current() else None,
        apply_settings,
        lambda: manager.connect(**settings()),
        manager.disconnect,
        GetQMaxMainWindow(),
    )
    _dialog.show()
