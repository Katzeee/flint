import threading
import time

from flint_bridge.execution.scheduling import CallbackQueue, QtMainThread, WorkerThread, dispatch_to_main_thread


def pump(app, until):
    deadline = time.monotonic() + 3
    while not until() and time.monotonic() < deadline:
        app.processEvents()
        time.sleep(0.001)
    assert until()


def test_worker_runs_callbacks_in_order_on_one_thread_until_closed():
    worker, calls = WorkerThread(), []
    finished, late = threading.Event(), threading.Event()
    worker.post(lambda: calls.append(("first", threading.get_ident())))
    worker.post(lambda: calls.append(("second", threading.get_ident())))
    worker.post(finished.set)
    worker.close()
    worker.post(late.set)
    assert finished.wait(3)
    assert [name for name, _ in calls] == ["first", "second"]
    assert calls[0][1] == calls[1][1] != threading.get_ident()
    assert not late.wait(0.05)


def test_queue_runs_only_what_was_posted_before_each_drain():
    queue, calls = CallbackQueue(), []
    queue.post(lambda: (calls.append(1), queue.post(lambda: calls.append(2))))
    queue.drain()
    assert calls == [1]
    queue.close()
    assert calls == [1, 2]


def test_qt_scheduler_runs_callbacks_posted_from_any_thread_on_the_application_thread(qapp):
    scheduler, calls = QtMainThread(), []
    worker = threading.Thread(target=lambda: scheduler.post(lambda: calls.append(threading.get_ident())))
    worker.start()
    worker.join(3)
    assert calls == []
    pump(qapp, lambda: bool(calls))
    assert calls == [threading.get_ident()]


def test_initialization_dispatch_uses_application_thread(qapp):
    calls = []
    worker = threading.Thread(target=lambda: dispatch_to_main_thread(lambda: calls.append(threading.get_ident())))
    worker.start()
    worker.join(3)
    pump(qapp, lambda: bool(calls))
    assert calls == [threading.get_ident()]
