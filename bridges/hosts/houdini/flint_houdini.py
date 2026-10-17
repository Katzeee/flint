"""Houdini user settings and connection controls for Flint Bridge."""

import json
from pathlib import Path

import hou
from flint_bridge import BridgeCreationError
from flint_bridge.houdini import manager

_dialog = None


def _settings_path():
    return Path(hou.homeHoudiniDirectory()) / "flint.json"


def settings():
    path = _settings_path()
    saved = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}
    return {"address": "127.0.0.1", "port": 6321, "name": "Houdini", "token": "", **saved}


def apply_settings(address, port, name, token):
    _settings_path().write_text(
        json.dumps({"address": address, "port": int(port), "name": name, "token": token}), encoding="utf-8"
    )


def initialize():
    try:
        manager.connect(**settings())
    except BridgeCreationError as error:
        print("Flint Bridge did not start: {}".format(error))


def show_settings():
    parent = hou.qt.mainWindow()
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
        parent,
    )
    _dialog.show()
