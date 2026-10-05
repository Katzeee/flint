import importlib.util
from pathlib import Path
import sys

import pytest


@pytest.fixture
def panel_module(monkeypatch, qapp):
    from PySide6 import QtCore, QtWidgets

    # Hosts ship PySide2; its API used here matches PySide6.
    monkeypatch.setitem(sys.modules, "PySide2", type(sys)("PySide2"))
    monkeypatch.setitem(sys.modules, "PySide2.QtCore", QtCore)
    monkeypatch.setitem(sys.modules, "PySide2.QtWidgets", QtWidgets)
    sys.modules["PySide2"].QtCore, sys.modules["PySide2"].QtWidgets = QtCore, QtWidgets
    path = Path(__file__).resolve().parents[1] / "flint_connection_panel.py"
    spec = importlib.util.spec_from_file_location("flint_connection_panel", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


SETTINGS = {"address": "127.0.0.1", "port": 6321, "name": "Maya", "enabled": True}


def retrying(message):
    return {"connection": {"state": "retrying",
                           "obstacle": {"kind": "unreachable", "message": message}},
            "busy": False, "settings": SETTINGS}


def test_an_obstacle_and_a_refused_action_are_shown_separately(panel_module):
    snapshot = {"value": retrying("connection refused")}

    def refuse(*_):
        raise RuntimeError("Bridge is executing host code")

    panel = panel_module.ConnectionPanel(
        SETTINGS, lambda: snapshot["value"], refuse, lambda: None)
    assert panel.status.text() == "Retrying"
    assert panel.warning_text.text() == "connection refused"

    panel.apply()
    assert panel.action_result.text() == "Bridge is executing host code"
    assert panel.warning_text.text() == "connection refused"

    # Polling replaces the status but leaves the action's result alone.
    snapshot["value"] = {"connection": {"state": "connected", "instance_id": "maya-1"},
                         "busy": False, "settings": SETTINGS}
    panel.refresh()
    assert panel.status.text() == "Connected"
    assert panel.warning.isHidden()
    assert panel.action_result.text() == "Bridge is executing host code"

    panel.name.textEdited.emit("Maya 2")
    assert panel.action_result.isHidden()


def test_without_a_bridge_apply_starts_one_and_reports_why_it_could_not(panel_module):
    attempts = []

    def start(*settings):
        attempts.append(settings)
        raise RuntimeError("Another Bridge already owns this process")

    panel = panel_module.ConnectionPanel(SETTINGS, lambda: None, start, lambda: None)
    assert panel.status.text() == "Stopped"
    assert panel.apply_button.isEnabled()
    assert not panel.retry_button.isEnabled()
    panel.apply()
    assert attempts == [("127.0.0.1", 6321, "Maya", True)]
    assert panel.action_result.text() == "Another Bridge already owns this process"
    assert panel.warning.isHidden()
