import pytest


@pytest.fixture
def panel_module(qapp):
    from flint_bridge.ui import connection_panel

    return connection_panel


SETTINGS = {"address": "127.0.0.1", "port": 6321, "name": "Maya"}


def retrying(message):
    return {
        "connection": {"state": "retrying", "obstacle": {"kind": "unreachable", "message": message}},
        "busy": False,
        "settings": SETTINGS,
    }


def test_an_obstacle_and_a_refused_action_are_shown_separately(panel_module):
    snapshot = {"value": retrying("connection refused")}
    disconnects = []
    execution_starts_on_disconnect = False

    def disconnect():
        disconnects.append(True)
        if execution_starts_on_disconnect:
            snapshot["value"] = {
                "connection": {"state": "stopped"},
                "busy": True,
                "settings": SETTINGS,
            }
            return False
        raise RuntimeError("Bridge is executing host code")

    panel = panel_module.ConnectionPanel(
        lambda: SETTINGS, lambda: snapshot["value"], lambda *_: None, lambda: None, disconnect
    )
    assert panel.status.text() == "Retrying"
    assert panel.warning_text.text() == "connection refused"

    panel.connection_button.click()
    assert panel.action_result.text() == "Bridge is executing host code"
    assert panel.warning_text.text() == "connection refused"

    # Polling replaces the status but leaves the action's result alone.
    snapshot["value"] = {
        "connection": {"state": "connected", "instance_id": "maya-1"},
        "busy": False,
        "settings": SETTINGS,
    }
    panel.refresh()
    assert panel.status.text() == "Connected"
    assert panel.warning.isHidden()
    assert panel.action_result.text() == "Bridge is executing host code"

    snapshot["value"]["busy"] = True
    panel.refresh()
    assert not panel.connection_button.isEnabled()
    panel.connection_button.click()
    assert len(disconnects) == 1

    snapshot["value"]["busy"] = False
    panel.refresh()
    assert panel.connection_button.isEnabled()
    # Execution may begin after the last refresh but before Disconnect reaches the manager.
    execution_starts_on_disconnect = True
    panel.connection_button.click()
    assert len(disconnects) == 2
    assert panel.status.text() == "Stopped"
    assert panel.action_result.text() == "Bridge is still executing host code"
    assert not panel.connection_button.isEnabled()

    snapshot["value"]["busy"] = False
    panel.refresh()
    assert panel.connection_button.isEnabled()
    assert panel.action_result.text() == "Bridge is still executing host code"
    panel.apply_button.click()
    assert panel.action_result.isHidden()


def test_apply_saves_without_reconnecting_and_connect_uses_saved_settings(panel_module):
    saved = SETTINGS.copy()
    snapshot = {"value": retrying("connection refused")}
    starts = []

    def save(address, port, name):
        saved.update(address=address, port=port, name=name)

    def disconnect():
        snapshot["value"] = None
        return True

    def connect():
        starts.append(saved.copy())
        snapshot["value"] = {
            "connection": {"state": "connecting"},
            "busy": False,
            "settings": saved.copy(),
        }

    panel = panel_module.ConnectionPanel(lambda: saved, lambda: snapshot["value"], save, connect, disconnect)
    panel.port.setValue(6330)
    panel.refresh()
    assert panel.pending.isHidden()
    assert panel.apply_button.text() == "Apply"
    assert panel.connection_button.text() == "Disconnect"
    panel.apply_button.click()
    assert saved["port"] == 6330
    assert snapshot["value"]["settings"]["port"] == 6321
    assert starts == []
    assert not panel.pending.isHidden()

    # Further edits are drafts; the hint compares saved settings with the connection.
    panel.port.setValue(6321)
    panel.refresh()
    assert not panel.pending.isHidden()
    panel.connection_button.click()
    panel.refresh()
    assert snapshot["value"] is None
    assert starts == []
    assert panel.pending.isHidden()
    assert panel.connection_button.text() == "Connect"
    panel.connection_button.click()
    assert starts[0]["port"] == 6330
    assert panel.pending.isHidden()
    assert panel.connection_button.text() == "Disconnect"
    assert not panel.apply_button.isHidden()
    assert not panel.connection_button.isHidden()

    # Saving while disconnected does not implicitly connect.
    panel.connection_button.click()
    panel.apply_button.click()
    assert saved["port"] == 6321
    assert snapshot["value"] is None
    assert len(starts) == 1
