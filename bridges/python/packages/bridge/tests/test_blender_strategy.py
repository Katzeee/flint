import sys
import threading
import time
from types import SimpleNamespace

from flint_bridge.execution.strategies.base import ExecutionStrategyClosedError
from flint_bridge.execution.strategies.blender import BlenderMainThreadExecutionStrategy
from flint_bridge.hosts.blender import create_strategy


class FakeTimers:
    def register(self, callback, first_interval, persistent):
        self.callback = callback
        self.first_interval = first_interval
        self.persistent = persistent
        self.registered_thread = threading.current_thread()


def test_blender_runs_worker_code_on_main_thread(monkeypatch):
    timers = FakeTimers()
    monkeypatch.setitem(sys.modules, "bpy", SimpleNamespace(app=SimpleNamespace(timers=timers)))
    strategy = create_strategy()
    assert timers.registered_thread is threading.main_thread()
    assert timers.persistent is True
    results = []
    worker = threading.Thread(target=lambda: results.append(
        strategy.run(lambda: threading.current_thread())))
    worker.start()
    deadline = time.monotonic() + 3
    while not strategy._pending and time.monotonic() < deadline:
        time.sleep(0.001)
    assert strategy._pending
    assert timers.callback() == timers.first_interval
    worker.join(3)
    assert not worker.is_alive()
    assert results == [threading.main_thread()]
    strategy.close()
    assert timers.callback() is None


def test_blender_close_cancels_queued_work():
    timers = FakeTimers()
    strategy = BlenderMainThreadExecutionStrategy(timers)
    errors, executed = [], []

    def worker():
        try:
            strategy.run(lambda: executed.append(True))
        except BaseException as error:
            errors.append(error)

    thread = threading.Thread(target=worker)
    thread.start()
    deadline = time.monotonic() + 3
    while not strategy._pending and time.monotonic() < deadline:
        time.sleep(0.001)
    assert strategy._pending
    strategy.close()
    thread.join(3)
    assert not thread.is_alive()
    assert len(errors) == 1 and isinstance(errors[0], ExecutionStrategyClosedError)
    assert timers.callback() is None
    assert not executed


def test_blender_connection_requires_main_thread(monkeypatch):
    monkeypatch.setitem(sys.modules, "bpy", SimpleNamespace(app=SimpleNamespace(timers=FakeTimers())))
    errors = []
    thread = threading.Thread(target=lambda: _connect_from_worker(errors))
    thread.start()
    thread.join(3)
    assert len(errors) == 1
    assert isinstance(errors[0], RuntimeError)
    assert "main thread" in str(errors[0])


def _connect_from_worker(errors):
    try:
        create_strategy()
    except BaseException as error:
        errors.append(error)
