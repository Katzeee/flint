import threading
from types import SimpleNamespace

import pytest

from flint_bridge import BridgeManager


@pytest.fixture
def integration(monkeypatch):
    class FakeBridge:
        def __init__(self, runner, host, address, port, name, enabled=True):
            self.runner = runner
            self.host, self.address, self.port = host, address, port
            self.name, self.enabled = name, enabled
            self.thread = SimpleNamespace(is_alive=lambda: True)
            self.instance_id = "instance-1"

        def apply_settings(self, address, port, name, enabled=True):
            self.address, self.port, self.name, self.enabled = address, port, name, enabled

        def check_running(self):
            pass

        def start(self):
            return self

        def wait_until_connected(self, timeout):
            return True

        def stop(self):
            self.runner.close()
            return True

    monkeypatch.setattr("flint_bridge.connection.bridge_manager.Bridge", FakeBridge)
    owner = BridgeManager("custom-host", lambda: SimpleNamespace(close=lambda: None), lambda callback: callback())
    yield owner
    owner.disconnect()


def test_failed_thread_start_releases_core_and_runner(monkeypatch, integration):
    from flint_bridge.connection import bridge as bridge_module

    released = []

    class Core:
        def __init__(self, config):
            pass

        def close(self):
            released.append("core")

    def fail_start(self):
        raise RuntimeError("could not start")

    monkeypatch.setattr(bridge_module, "NativeCore", Core)
    monkeypatch.setattr("flint_bridge.connection.bridge_manager.Bridge", bridge_module.Bridge)
    monkeypatch.setattr(threading.Thread, "start", fail_start)
    owner = BridgeManager(integration.host, lambda: SimpleNamespace(close=lambda: released.append("runner")),
                       integration.dispatch_initialization)
    with pytest.raises(RuntimeError, match="could not start"):
        owner.connect()
    assert sorted(released) == ["core", "runner"]
    assert owner.current() is None


def test_another_host_cannot_replace_or_control_the_existing_bridge(integration):
    first = integration.connect()
    other = BridgeManager("another-host", lambda: pytest.fail("Created another host's runner"),
                       integration.dispatch_initialization)
    with pytest.raises(RuntimeError):
        other.connect()
    with pytest.raises(RuntimeError):
        other.configure(port=6500)
    assert other.current() is None
    assert other.disconnect()
    assert integration.current() is first
    assert first.port == 6321


def test_registration_wait_uses_the_budget_remaining_after_initialization(monkeypatch, integration):
    first = integration.connect()
    waits = []
    first.wait_until_connected = lambda timeout: waits.append(timeout) or True
    times = iter((100, 104, 106))
    monkeypatch.setattr("flint_bridge.connection.bridge_manager.time",
                        SimpleNamespace(monotonic=lambda: next(times)))
    assert integration.attach(timeout=10) == first.instance_id
    assert waits == [4]


def test_attach_timeout_reports_running_initialization_without_discarding_it(monkeypatch, integration):
    started, release = threading.Event(), threading.Event()
    workers = []
    configure = BridgeManager.configure

    def delayed_configure(self, *args):
        started.set()
        assert release.wait(3)
        return configure(self, *args)

    def dispatch(callback):
        worker = threading.Thread(target=callback)
        workers.append(worker)
        worker.start()
        assert started.wait(3)

    monkeypatch.setattr(BridgeManager, "configure", delayed_configure)
    owner = BridgeManager(integration.host, integration.create_runner, dispatch)
    try:
        with pytest.raises(TimeoutError, match="still in progress"):
            owner.attach(timeout=0)
    finally:
        release.set()
        for worker in workers:
            worker.join(3)
    assert owner.current() is not None
