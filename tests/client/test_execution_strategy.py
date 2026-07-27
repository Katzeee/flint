import threading
import time

import pytest

from python_bridge_mcp.client.code_executor import CodeExecutor
from python_bridge_mcp.client.code_runner import CodeRunner
from python_bridge_mcp.client.bootstrap import start_control_client_service
from python_bridge_mcp.client.discovery import DiscoveryClient
from python_bridge_mcp.client.execution_strategy._queued_dispatcher import QueuedDispatcher
from python_bridge_mcp.client.execution_strategy import (
    BlenderMainThreadExecutionStrategy,
    DirectExecutionStrategy,
    ExecutionStrategyClosedError,
    QtMainThreadExecutionStrategy,
)
from python_bridge_mcp.shared.instance_control_models import InstanceExecStatus

from conftest import wait_for


class _FakeTimers:
    def __init__(self) -> None:
        self.callback = None
        self.options = {}
        self.unregister_threads = []

    def register(self, callback, **options) -> None:
        self.callback = callback
        self.options = options

    def unregister(self, callback) -> None:
        assert callback is self.callback
        self.unregister_threads.append(threading.current_thread())
        self.callback = None

    def is_registered(self, callback) -> bool:
        return callback is self.callback


class _FakeApp:
    def __init__(self) -> None:
        self.timers = _FakeTimers()


class _FakeBpy:
    def __init__(self) -> None:
        self.app = _FakeApp()


class _FakeSignal:
    def connect(self, callback) -> None:
        self.callback = callback

    def emit(self, payload) -> None:
        self.callback(payload)


class _FakeQtCore:
    _thread = object()

    class QObject:
        def moveToThread(self, thread) -> None:
            pass

    class QThread:
        @staticmethod
        def currentThread():
            return _FakeQtCore._thread

    class QWaitCondition:
        pass

    class QMutex:
        pass

    @staticmethod
    def Signal(payload_type):
        return _FakeSignal()


class _FakeQtWidgets:
    class QApplication:
        @staticmethod
        def instance():
            return _FakeQtWidgets.QApplication()

        def thread(self):
            return _FakeQtCore._thread


class _FakeQt:
    QtCore = _FakeQtCore
    QtWidgets = _FakeQtWidgets


def _pump_until_thread_stops(timers, thread, timeout=2.0) -> bool:
    deadline = time.monotonic() + timeout
    while thread.is_alive() and time.monotonic() < deadline:
        callback = timers.callback
        if callback is not None:
            callback()
        thread.join(timeout=0.01)
    return not thread.is_alive()


def test_queued_dispatcher_owns_execution_and_cancellation() -> None:
    dispatcher = QueuedDispatcher()
    completed = dispatcher.submit(lambda: 42)
    cancelled = dispatcher.submit(lambda: None)

    assert dispatcher.execute_pending(limit=1) == 1
    assert completed.wait() == 42
    assert dispatcher.cancel_pending() == 1
    with pytest.raises(ExecutionStrategyClosedError):
        cancelled.wait()


def test_blender_strategy_executes_worker_request_on_main_thread() -> None:
    bpy = _FakeBpy()
    strategy = BlenderMainThreadExecutionStrategy(bpy_module=bpy)
    result = []
    errors = []

    def worker() -> None:
        try:
            result.append(strategy.run(lambda: threading.current_thread()))
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=worker)
    thread.start()

    assert _pump_until_thread_stops(bpy.app.timers, thread)

    assert not errors
    assert result == [threading.main_thread()]
    assert bpy.app.timers.options == {"first_interval": 0.0, "persistent": True}
    strategy.close()
    strategy.close()
    assert bpy.app.timers.unregister_threads == [threading.main_thread()]


def test_blender_strategy_close_rejects_pending_work() -> None:
    bpy = _FakeBpy()
    strategy = BlenderMainThreadExecutionStrategy(bpy_module=bpy)
    errors = []
    worker_started = threading.Event()

    def worker() -> None:
        worker_started.set()
        try:
            strategy.run(lambda: None)
        except BaseException as exc:
            errors.append(exc)

    thread = threading.Thread(target=worker)
    thread.start()
    assert worker_started.wait(timeout=2)

    strategy.close()
    thread.join(timeout=2)

    assert len(errors) == 1
    assert isinstance(errors[0], ExecutionStrategyClosedError)
    assert bpy.app.timers.callback is None


def test_blender_strategy_background_close_defers_timer_removal() -> None:
    bpy = _FakeBpy()
    strategy = BlenderMainThreadExecutionStrategy(bpy_module=bpy)

    thread = threading.Thread(target=strategy.close)
    thread.start()
    thread.join(timeout=2)

    assert bpy.app.timers.unregister_threads == []
    assert bpy.app.timers.callback() is None


def test_direct_strategy_close_is_terminal_and_idempotent() -> None:
    strategy = DirectExecutionStrategy()

    strategy.close()
    strategy.close()

    with pytest.raises(ExecutionStrategyClosedError):
        strategy.run(lambda: None)


def test_qt_strategy_close_is_terminal() -> None:
    strategy = QtMainThreadExecutionStrategy(qt=_FakeQt())

    strategy.close()

    with pytest.raises(ExecutionStrategyClosedError):
        strategy.run(lambda: None)


def test_blender_runner_uses_strategy_for_code_execution() -> None:
    bpy = _FakeBpy()
    runner = CodeRunner(
        CodeExecutor(),
        BlenderMainThreadExecutionStrategy(bpy_module=bpy),
    )
    results = []

    thread = threading.Thread(
        target=lambda: results.append(runner.execute("exec-1", "value = 42"))
    )
    thread.start()
    assert wait_for(
        lambda: bpy.app.timers.callback is not None,
        timeout=2.0,
        poll=0.01,
    )
    bpy.app.timers.callback()
    thread.join(timeout=2)

    assert results[0].status == InstanceExecStatus.SUCCEEDED
    runner.close()


def test_blender_strategy_must_be_created_on_main_thread() -> None:
    errors = []

    def worker() -> None:
        with pytest.raises(RuntimeError) as exc_info:
            BlenderMainThreadExecutionStrategy(bpy_module=_FakeBpy())
        errors.append(exc_info.value)

    thread = threading.Thread(target=worker)
    thread.start()
    thread.join(timeout=2)

    assert len(errors) == 1


def test_code_runner_can_receive_an_explicit_strategy() -> None:
    class _RecordingStrategy(DirectExecutionStrategy):
        def __init__(self) -> None:
            super().__init__()
            self.calls = 0

        def run(self, func):
            self.calls += 1
            return super().run(func)

    strategy = _RecordingStrategy()
    runner = CodeRunner(CodeExecutor(), strategy)

    result = runner.execute("exec-1", "answer = 42")

    assert result.status == InstanceExecStatus.SUCCEEDED
    assert strategy.calls == 1


def test_code_runner_rejects_empty_strategy() -> None:
    with pytest.raises(TypeError, match="strategy is required"):
        CodeRunner(CodeExecutor(), None)  # type: ignore[arg-type]


def test_control_client_service_rejects_empty_runner() -> None:
    with pytest.raises(TypeError, match="runner is required"):
        start_control_client_service(
            name_hint="test",
            instance_name="Test",
            runner=None,  # type: ignore[arg-type]
        )


def test_discovery_client_owns_runner_lifecycle() -> None:
    runner = CodeRunner(CodeExecutor(), DirectExecutionStrategy())
    client = DiscoveryClient(
        name_hint="test",
        instance_name="Test",
        runner=runner,
    )

    client.stop()

    with pytest.raises(ExecutionStrategyClosedError):
        runner.execute("exec-after-stop", "value = 42")
