"""Translate the core's host callbacks into Python calls and completions."""
from concurrent.futures import Future
import itertools
import json
import threading
import traceback

from .execution_binding import ExecutionBinding, POST, PREPARE, RUN, DISCARD, RELEASE

# The core may call back until it releases the registration. A released adapter
# outlives its own release call, whose callback object must not be freed mid-call.
_execution_adapters = {}
_released_execution_adapters = []


class _StepWrapper:
    """One completion owed to the core; output stops when it completes."""

    def __init__(self, bridge_api, raw):
        self._bridge_api, self.raw = bridge_api, raw
        self._lock = threading.Lock()

    def write(self, text, stderr):
        data = text.encode("utf-8", "replace")
        out, err = (b"", data) if stderr else (data, b"")
        with self._lock:
            if self.raw is not None:
                self._bridge_api.flint_step_output(self.raw, out, len(out), err, len(err))

    def succeed(self, result_id=0):
        raw = self._take()
        if raw is not None:
            self._bridge_api.flint_step_succeed(raw, result_id)

    def fail(self, trace):
        raw = self._take()
        if raw is not None:
            self._bridge_api.flint_step_fail(raw, trace.encode("utf-8", "replace"), None)

    def _take(self):
        with self._lock:
            raw, self.raw = self.raw, None
        return raw


class _Output:
    def __init__(self, step_wrapper, stderr):
        self._step_wrapper, self._stderr = step_wrapper, stderr

    def write(self, text):
        if not isinstance(text, str):
            raise TypeError("write() argument must be str, not " + type(text).__name__)
        self._step_wrapper.write(text, self._stderr)
        return len(text)

    def flush(self):
        pass

    def isatty(self):
        return False


def _identity(request):
    return request


class ExecutionAdapter:
    """Holds Python references for the core; a value or Future completes each step."""

    def __init__(self, bridge_api, execution_capabilities):
        self._bridge_api = bridge_api
        self._prepare = getattr(execution_capabilities.executor, "prepare", _identity)
        self._run = execution_capabilities.executor.run
        self._scheduler = execution_capabilities.scheduler
        self._prepared_results = {}
        self._result_ids = itertools.count(1)
        self.execution_binding = ExecutionBinding(
            id(self), POST(self._on_post), PREPARE(self._on_prepare), RUN(self._on_run),
            DISCARD(self._on_discard), RELEASE(self._on_release))
        _execution_adapters[id(self)] = self

    def _on_post(self, context, ticket):
        try:
            self._scheduler.post(lambda: self._bridge_api.flint_ticket_run(ticket))
            return True
        except BaseException:
            traceback.print_exc()
            return False

    def _on_prepare(self, context, request, raw):
        step_wrapper = _StepWrapper(self._bridge_api, raw)

        def on_prepared(prepared_result):
            result_id = next(self._result_ids)
            self._prepared_results[result_id] = prepared_result
            step_wrapper.succeed(result_id)

        self._call(step_wrapper, lambda: self._prepare(json.loads(request.decode("utf-8"))), on_prepared)

    def _on_run(self, context, result_id, raw):
        step_wrapper = _StepWrapper(self._bridge_api, raw)
        prepared_result = self._prepared_results.pop(result_id)
        self._call(
            step_wrapper,
            lambda: self._run(prepared_result, _Output(step_wrapper, False), _Output(step_wrapper, True)),
            lambda value: step_wrapper.succeed())

    def _on_discard(self, context, result_id):
        self._prepared_results.pop(result_id, None)

    def _on_release(self, context):
        _released_execution_adapters[:] = [_execution_adapters.pop(context, None)]

    @staticmethod
    def _call(step_wrapper, invoke, done):
        try:
            result = invoke()
        except BaseException:
            step_wrapper.fail(traceback.format_exc())
            return
        if isinstance(result, Future):
            result.add_done_callback(lambda future: ExecutionAdapter._settle(step_wrapper, future, done))
        else:
            done(result)

    @staticmethod
    def _settle(step_wrapper, future, done):
        try:
            value = future.result()
        except BaseException as error:
            step_wrapper.fail("".join(traceback.format_exception(type(error), error, error.__traceback__)))
            return
        done(value)
