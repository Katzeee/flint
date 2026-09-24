"""3ds Max user settings and connection controls for Flint Bridge."""
from pymxs import runtime as rt
from pathlib import Path
import flint_bridge

_SECTION = "Flint Bridge"
_DEFAULTS = {"address": "127.0.0.1", "port": 6321,
             "name": "3ds Max", "enabled": True}
_KEYS = {"address": "RegistryAddress", "port": "RegistryPort",
         "name": "InstanceName", "enabled": "Enabled"}
_dialog = None


def _ini():
    return str(Path(str(rt.getDir(rt.Name("maxData")))) / "FlintBridge.ini")


def settings():
    values = {}
    for field, default in _DEFAULTS.items():
        value = str(rt.getINISetting(_ini(), _SECTION, _KEYS[field]))
        values[field] = value if value else default
    values["port"] = int(values["port"])
    values["enabled"] = str(values["enabled"]).lower() not in ("0", "false")
    return values


def apply_settings(address, port, name, enabled):
    port = int(port)
    enabled = bool(enabled)
    bridge = flint_bridge.configure(
        "max", address=address, port=port, name=name, enabled=enabled)
    for field, value in {"address": address, "port": port, "name": name,
                         "enabled": int(enabled)}.items():
        rt.setINISetting(_ini(), _SECTION, _KEYS[field], str(value))
    return bridge


def reconnect():
    return flint_bridge.reconnect()


def initialize():
    flint_bridge.connect(host="max", **settings())


def show_settings():
    from qtmax import GetQMaxMainWindow
    from flint_connection_panel import ConnectionPanel

    global _dialog
    if _dialog is not None:
        _dialog.close()
    _dialog = ConnectionPanel(
        settings(), lambda: flint_bridge.current().status if flint_bridge.current() else None,
        apply_settings, reconnect, GetQMaxMainWindow())
    _dialog.show()
