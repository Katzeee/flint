import threading
from types import SimpleNamespace

from flint_bridge.connection import bridge as bridge_module


def test_stop_during_poll_rejects_undispatched_work_and_releases_resources(monkeypatch):
    polling, stopped = threading.Event(), threading.Event()
    reports, released, executed = [], [], []

    class Core:
        busy = False

        def __init__(self, config):
            pass

        def poll(self, timeout):
            if timeout:
                polling.set()
                assert stopped.wait(3)
                return {"request_id": "pending"}
            return None

        def stop(self):
            stopped.set()

        def report_execution(self, report):
            reports.append(report)

        def close(self):
            released.append("core")

    monkeypatch.setattr(bridge_module, "NativeCore", Core)
    runner = SimpleNamespace(close=lambda: released.append("runner"),
                             execute=lambda *args: executed.append(args))
    bridge = bridge_module.Bridge(runner, "test-host", "127.0.0.1", 6321, "test")
    bridge.start()
    try:
        assert polling.wait(3)
    finally:
        finished = bridge.stop()
    assert finished
    assert executed == []
    assert sorted(released) == ["core", "runner"]
    assert len(reports) == 1
    assert reports[0]["request_id"] == "pending"
    assert reports[0]["succeeded"] is False
