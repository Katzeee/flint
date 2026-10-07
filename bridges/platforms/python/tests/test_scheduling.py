import threading
import time

import pytest

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
    worker.post(lambda: calls.append(("first", threading.current_thread())))
    worker.post(lambda: calls.append(("second", threading.current_thread())))
    worker.post(finished.set)
    worker.close()
    worker.post(late.set)
    assert finished.wait(3)
    thread = calls[0][1]
    thread.join(3)
    assert not thread.is_alive()
    assert [name for name, _ in calls] == ["first", "second"]
    assert calls[0][1] is calls[1][1] and thread is not threading.current_thread()
    assert not late.is_set()


def test_queue_runs_only_what_was_posted_before_each_drain():
    queue, calls = CallbackQueue(), []
    queue.post(lambda: (calls.append(1), queue.post(lambda: calls.append(2))))
    queue.drain()
    assert calls == [1]
    queue.close()
    assert calls == [1, 2]


@pytest.mark.parametrize("initialization", [False, True], ids=["execution", "initialization"])
def test_qt_dispatch_runs_on_the_application_thread(qapp, initialization):
    post = dispatch_to_main_thread if initialization else QtMainThread().post
    calls = []
    worker = threading.Thread(target=lambda: post(lambda: calls.append(threading.get_ident())))
    worker.start()
    worker.join(3)
    assert calls == []
    pump(qapp, lambda: bool(calls))
    assert calls == [threading.get_ident()]
