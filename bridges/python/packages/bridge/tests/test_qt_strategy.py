import threading
import time
from typing import Callable, List, Tuple

import PySide6
from PySide6 import QtCore, QtWidgets

from flint_bridge.execution.strategies import QtMainThreadExecutionStrategy
from flint_bridge.execution.strategies import ExecutionStrategyClosedError


def _run_from_worker(
    app: QtWidgets.QApplication,
    func: Callable[[], object],
    timeout: float = 3.0,
) -> Tuple[List[object], List[BaseException]]:
    results: List[object] = []
    errors: List[BaseException] = []

    def worker() -> None:
        try:
            results.append(func())
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=worker, daemon=True)
    thread.start()
    deadline = time.monotonic() + timeout
    while thread.is_alive() and time.monotonic() < deadline:
        app.processEvents(QtCore.QEventLoop.AllEvents, 10)
        thread.join(timeout=0.001)

    assert not thread.is_alive(), "Qt worker did not finish before timeout"
    return results, errors


def test_executes_worker_request_on_application_thread(qapp) -> None:
    strategy = QtMainThreadExecutionStrategy()

    results, errors = _run_from_worker(
        qapp,
        lambda: strategy.run(
            lambda: QtCore.QThread.currentThread() is qapp.thread()
        ),
    )

    assert errors == []
    assert results == [True]


def test_returns_values_and_propagates_exceptions(qapp) -> None:
    strategy = QtMainThreadExecutionStrategy(qt=PySide6)

    results, errors = _run_from_worker(qapp, lambda: strategy.run(lambda: 42))
    assert results == [42]
    assert errors == []

    def fail() -> None:
        raise ValueError("boom")

    results, errors = _run_from_worker(qapp, lambda: strategy.run(fail))
    assert results == []
    assert len(errors) == 1
    assert isinstance(errors[0], ValueError)
    assert str(errors[0]) == "boom"


def test_handles_repeated_worker_dispatch(qapp) -> None:
    strategy = QtMainThreadExecutionStrategy(qt=PySide6)

    for value in range(50):
        results, errors = _run_from_worker(
            qapp,
            lambda value=value: strategy.run(lambda: value),
        )
        assert errors == []
        assert results == [value]


def test_close_cancels_queued_work_before_ui_dispatch(qapp):
    strategy = QtMainThreadExecutionStrategy()
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
    qapp.processEvents()
    assert not executed
