import threading
from types import SimpleNamespace

import pytest

from flint_bridge import BridgeManager


@pytest.fixture
def integration(monkeypatch):
    class FakeBridge:
        def __init__(self, capabilities, host, address, port, name, enabled=True):
            self.capabilities = capabilities
            self.host, self.address, self.port = host, address, port
            self.name, self.enabled = name, enabled
            self.instance_id = "instance-1"

        def apply_settings(self, address, port, name, enabled=True):
            self.address, self.port, self.name, self.enabled = address, port, name, enabled

        def check_running(self):
            pass

        def wait_until_connected(self, timeout):
            return True

        def stop(self):
            return True

    monkeypatch.setattr("flint_bridge.connection.bridge_manager.Bridge", FakeBridge)
    owner = BridgeManager("custom-host", object, lambda callback: callback())
    yield owner
    owner.disconnect()


def test_another_host_cannot_replace_or_control_the_existing_bridge(integration):
    first = integration.connect()
    other = BridgeManager("another-host", lambda: pytest.fail("Created another host's capabilities"),
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
    owner = BridgeManager(integration.host, integration.create_execution, dispatch)
    try:
        with pytest.raises(TimeoutError, match="still in progress"):
            owner.attach(timeout=0)
    finally:
        release.set()
        for worker in workers:
            worker.join(3)
    assert owner.current() is not None
