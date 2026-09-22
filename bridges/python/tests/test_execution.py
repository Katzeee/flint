import pytest
from flint_bridge.execution.executor import CodeExecutor
from flint_bridge.execution.runner import CodeRunner
from flint_bridge.execution.strategies import DirectExecutionStrategy, ExecutionStrategyClosedError
from flint_bridge.execution.buffer import ThreadSafeTextBuffer
from flint_bridge import connect, disconnect


def test_namespace_output_errors_and_closed_runner():
    runner = CodeRunner(CodeExecutor(), DirectExecutionStrategy())
    out, err = ThreadSafeTextBuffer(), ThreadSafeTextBuffer()
    assert runner.execute("1", "answer = 42").status.value == "succeeded"
    assert runner.execute("2", "import sys\nprint(answer)\nprint('error stream', file=sys.stderr)", out, err).status.value == "succeeded"
    assert out.getvalue() == "42\n" and err.getvalue() == "error stream\n"
    assert "SyntaxError" in runner.execute("3", "if :").traceback
    assert "SystemExit" in runner.execute("4", "raise SystemExit(0)").traceback
    runner.close()
    with pytest.raises(ExecutionStrategyClosedError):
        runner.execute("5", "print('must not run')")


def test_explicit_endpoint_change_does_not_replace_running_bridge():
    bridge = connect("python", port=1)
    try:
        assert connect("python", port=1) is bridge
        with pytest.raises(RuntimeError, match="Disconnect"):
            connect("python", port=2)
    finally:
        assert disconnect()
    assert not bridge.thread.is_alive()


def test_unknown_host_does_not_create_bridge():
    with pytest.raises(ValueError, match="Unsupported host"):
        connect("not-a-host")
