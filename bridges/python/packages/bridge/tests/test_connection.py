from types import SimpleNamespace

import pytest

from flint_bridge import attach, configure, connect, disconnect


def test_explicit_endpoint_change_does_not_replace_running_bridge(monkeypatch):
    class FakeBridge:
        def __init__(self, runner, host, address, port, name, enabled=True):
            self.runner = runner
            self.host, self.address, self.port = host, address, port
            self.name, self.enabled = name, enabled
            self.thread = SimpleNamespace(is_alive=lambda: True)

        def apply_settings(self, address, port, name, enabled=True):
            self.address, self.port, self.name, self.enabled = address, port, name, enabled

        def start(self):
            return self

        def stop(self):
            self.runner.close()
            return True

    monkeypatch.setattr("flint_bridge.connection.service.Bridge", FakeBridge)
    bridge = connect("python", port=1)
    try:
        assert connect("python", port=1) is bridge
        with pytest.raises(RuntimeError, match="Disconnect"):
            connect("python", port=2)
        assert configure("python", port=2, name="新的名称", enabled=False) is bridge
        assert (bridge.port, bridge.name, bridge.enabled) == (2, "新的名称", False)
    finally:
        assert disconnect()


def test_unknown_host_does_not_create_bridge():
    with pytest.raises(ValueError, match="Unsupported host"):
        connect("not-a-host")


def test_failed_start_releases_host_dispatch(monkeypatch):
    closed = []
    strategy = SimpleNamespace(close=lambda: closed.append(True))
    monkeypatch.setattr("flint_bridge.hosts.strategy_for", lambda host: strategy)

    class FailingBridge:
        def __init__(self, runner, host, address, port, name, enabled=True):
            pass

        def start(self):
            raise RuntimeError("could not start")

    monkeypatch.setattr("flint_bridge.connection.service.Bridge", FailingBridge)
    with pytest.raises(RuntimeError, match="could not start"):
        connect("blender")
    assert closed == [True]


def _connected_bridge_factory(record):
    class FakeBridge:
        def __init__(self, runner, host, address, port, name, enabled=True):
            self.runner = runner
            self.host, self.address, self.port = host, address, port
            self.name, self.enabled = name, enabled
            self.thread = SimpleNamespace(is_alive=lambda: True)
            self.instance_id = "instance-1"

        def apply_settings(self, address, port, name, enabled=True):
            record.append(("apply", address, port, name, enabled))
            self.address, self.port, self.name, self.enabled = address, port, name, enabled

        def start(self):
            return self

        def wait_until_connected(self, timeout=10):
            return True

        def stop(self):
            self.runner.close()
            return True

    return FakeBridge


def test_attach_marshals_onto_the_host_main_thread_and_returns_the_instance(monkeypatch):
    scheduled = []
    monkeypatch.setattr("flint_bridge.hosts.strategy_for", lambda host: SimpleNamespace(close=lambda: None))
    monkeypatch.setattr("flint_bridge.connection.service.Bridge", _connected_bridge_factory([]))
    # A real host runs the callback later on its main thread; capture and run it.
    monkeypatch.setattr("flint_bridge.hosts.enter_main_thread",
                        lambda host, callback: scheduled.append(callback) or callback())
    try:
        assert attach("maya", port=6400, name="Injected") == "instance-1"
        assert len(scheduled) == 1
    finally:
        assert disconnect()


def test_attach_repoints_an_existing_bridge_so_the_latest_configuration_wins(monkeypatch):
    applied = []
    monkeypatch.setattr("flint_bridge.hosts.strategy_for", lambda host: SimpleNamespace(close=lambda: None))
    monkeypatch.setattr("flint_bridge.connection.service.Bridge", _connected_bridge_factory(applied))
    monkeypatch.setattr("flint_bridge.hosts.enter_main_thread", lambda host, callback: callback())
    bridge = connect("maya", port=6400)
    try:
        assert attach("maya", port=6500, name="Reattached") == "instance-1"
        assert (bridge.port, bridge.name) == (6500, "Reattached")
        assert applied and applied[-1][2:4] == (6500, "Reattached")
    finally:
        assert disconnect()
