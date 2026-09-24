import io
import sys
import threading

import pytest
from flint_bridge.execution.executor import CodeExecutor
from flint_bridge.execution.runner import CodeRunner
from flint_bridge.execution.strategies import DirectExecutionStrategy, ExecutionStrategyClosedError


@pytest.fixture
def runner():
    runner = CodeRunner(CodeExecutor(), DirectExecutionStrategy())
    yield runner
    runner.close()


def test_namespace_persists_between_executions(runner):
    out = io.StringIO()
    assert runner.execute("1", "answer = 42").status.value == "succeeded"
    assert runner.execute("2", "print(answer)", out).status.value == "succeeded"
    assert out.getvalue() == "42\n"


def test_stdout_and_stderr_are_captured_separately(runner):
    out, err = io.StringIO(), io.StringIO()
    result = runner.execute("1", "import sys\nprint('output')\nprint('error stream', file=sys.stderr)", out, err)
    assert result.status.value == "succeeded"
    assert out.getvalue() == "output\n"
    assert err.getvalue() == "error stream\n"


@pytest.mark.parametrize("source, diagnostic", [
    ("raise ValueError('EXPECTED')", "ValueError: EXPECTED"),
    ("if :", "SyntaxError"),
    ("raise SystemExit(0)", "SystemExit"),
])
def test_failures_are_reported_as_tracebacks(runner, source, diagnostic):
    result = runner.execute("1", source)
    assert result.status.value == "failed"
    assert diagnostic in result.traceback


def test_closed_runner_rejects_execution(runner):
    runner.close()
    with pytest.raises(ExecutionStrategyClosedError):
        runner.execute("1", "print('must not run')")


def test_execution_does_not_capture_another_threads_output(monkeypatch):
    original = io.StringIO()
    monkeypatch.setattr(sys, "stdout", original)
    ready, resume = threading.Event(), threading.Event()
    out = io.StringIO()
    runner = CodeRunner(CodeExecutor({"ready": ready, "resume": resume}), DirectExecutionStrategy())
    results = []
    worker = threading.Thread(target=lambda: results.append(runner.execute(
        "1", "ready.set()\nresume.wait()\nprint('execution output')", out)))
    worker.start()
    assert ready.wait(3)
    print("unrelated output")
    resume.set()
    worker.join(3)
    assert not worker.is_alive()
    assert results[0].status.value == "succeeded"
    assert out.getvalue() == "execution output\n"
    assert original.getvalue() == "unrelated output\n"
