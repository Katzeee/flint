import io
import sys
import threading

from flint_bridge.execution.executor import CodeExecutor


def test_execution_does_not_capture_another_threads_output(monkeypatch):
    original_out, original_err = io.StringIO(), io.StringIO()
    out, err = io.StringIO(), io.StringIO()
    monkeypatch.setattr(sys, "stdout", original_out)
    monkeypatch.setattr(sys, "stderr", original_err)
    ready, resume = threading.Event(), threading.Event()
    executor = CodeExecutor({"ready": ready, "resume": resume})
    prepared_result = executor.prepare(
        {
            "code": "import sys\nready.set()\nassert resume.wait(3)\n"
            "print('execution output')\nprint('execution error', file=sys.stderr)"
        }
    )
    failures = []

    def run():
        try:
            executor.run(prepared_result, out, err)
        except BaseException as error:
            failures.append(error)

    worker = threading.Thread(target=run, daemon=True)
    worker.start()
    try:
        assert ready.wait(3)
        print("unrelated output")
        print("unrelated error", file=sys.stderr)
    finally:
        resume.set()
        worker.join(3)
    assert not worker.is_alive()
    assert failures == []
    assert out.getvalue() == "execution output\n"
    assert err.getvalue() == "execution error\n"
    assert original_out.getvalue() == "unrelated output\n"
    assert original_err.getvalue() == "unrelated error\n"
