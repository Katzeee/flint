from types import SimpleNamespace

import pytest

from flint_bridge import connect, disconnect


def test_explicit_endpoint_change_does_not_replace_running_bridge(monkeypatch):
    class FakeBridge:
        def __init__(self, runner, host, address, port, name):
            self.runner = runner
            self.host, self.address, self.port = host, address, port
            self.thread = SimpleNamespace(is_alive=lambda: True)

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
        def __init__(self, runner, host, address, port, name):
            pass

        def start(self):
            raise RuntimeError("could not start")

    monkeypatch.setattr("flint_bridge.connection.service.Bridge", FailingBridge)
    with pytest.raises(RuntimeError, match="could not start"):
        connect("blender")
    assert closed == [True]
