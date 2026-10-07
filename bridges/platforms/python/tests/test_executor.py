import io
import sys
import threading

import pytest
from flint_bridge.execution.executor import CodeExecutor


def execute(executor, code, out=None, err=None):
    prepared = executor.prepare({"code": code, "filename": "test-executor.py"})
    executor.run(prepared, out if out is not None else io.StringIO(), err if err is not None else io.StringIO())


def test_namespace_persists_between_executions():
    executor, out = CodeExecutor(), io.StringIO()
    execute(executor, "answer = 42")
    execute(executor, "print(answer)", out)
    assert out.getvalue() == "42\n"


def test_stdout_and_stderr_are_captured_separately():
    out, err = io.StringIO(), io.StringIO()
    execute(CodeExecutor(), "import sys\nprint('output')\nprint('error stream', file=sys.stderr)", out, err)
    assert out.getvalue() == "output\n"
    assert err.getvalue() == "error stream\n"


def test_syntax_errors_fail_preparation_before_anything_runs():
    with pytest.raises(SyntaxError):
        CodeExecutor().prepare({"code": "if :"})


@pytest.mark.parametrize("source, exception", [
    ("raise ValueError('EXPECTED')", ValueError), ("raise SystemExit(0)", SystemExit),
])
def test_run_propagates_failures_to_the_binding(source, exception):
    with pytest.raises(exception):
        execute(CodeExecutor(), source)


def test_execution_does_not_capture_another_threads_output(monkeypatch):
    original, out = io.StringIO(), io.StringIO()
    monkeypatch.setattr(sys, "stdout", original)
    ready, resume = threading.Event(), threading.Event()
    executor = CodeExecutor({"ready": ready, "resume": resume})
    worker = threading.Thread(target=lambda: execute(executor,
        "ready.set()\nresume.wait()\nprint('execution output')", out))
    worker.start()
    assert ready.wait(3)
    print("unrelated output")
    resume.set()
    worker.join(3)
    assert not worker.is_alive()
    assert out.getvalue() == "execution output\n"
    assert original.getvalue() == "unrelated output\n"
