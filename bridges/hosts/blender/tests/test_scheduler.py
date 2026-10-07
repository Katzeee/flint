import sys
import threading
from types import SimpleNamespace
from flint_bridge.blender import create_scheduler


class Timers:
    def register(self, callback, first_interval, persistent):
        self.callback = callback
        self.persistent = persistent

    def is_registered(self, callback):
        return self.callback is callback

    def unregister(self, callback):
        assert self.callback is callback
        self.callback = None


def test_timer_drains_posted_callbacks_until_closed(monkeypatch):
    timers, calls = Timers(), []
    monkeypatch.setitem(sys.modules, "bpy", SimpleNamespace(app=SimpleNamespace(timers=timers)))
    scheduler = create_scheduler()
    assert timers.persistent
    worker = threading.Thread(target=scheduler.post, args=(lambda: calls.append(threading.get_ident()),))
    worker.start()
    worker.join(3)
    assert calls == []
    assert timers.callback() == 0.02
    assert calls == [threading.get_ident()]
    scheduler.post(lambda: calls.append("pending"))
    scheduler.close()
    assert timers.callback is None
    assert calls[-1] == "pending"


def test_connection_requires_main_thread():
    errors = []

    def create():
        try:
            create_scheduler()
        except RuntimeError as error:
            errors.append(str(error))

    worker = threading.Thread(target=create)
    worker.start()
    worker.join(3)
    assert len(errors) == 1 and "main thread" in errors[0]
