"""Translate the core's host callbacks into Python calls and completions."""
from concurrent.futures import Future
import itertools
import json
import traceback

from .execution_binding import ExecutionBinding, POST, PREPARE, RUN, DISCARD, RELEASE

# The core may call back until it releases the registration. A released adapter
# outlives its own release call, whose callback object must not be freed mid-call.
_execution_adapters = {}
_released_execution_adapters = []


class _Output:
    def __init__(self, bridge_api, step, stderr):
        self._bridge_api, self._step, self._stderr = bridge_api, step, stderr

    def write(self, text):
        if not isinstance(text, str):
            raise TypeError("write() argument must be str, not " + type(text).__name__)
        data = text.encode("utf-8", "replace")
        out, err = (b"", data) if self._stderr else (data, b"")
        self._bridge_api.flint_step_output(self._step, out, len(out), err, len(err))
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

    def _on_post(self, context, step):
        try:
            self._scheduler.post(lambda: self._bridge_api.flint_step_run(step))
            return True
        except BaseException:
            traceback.print_exc()
            return False

    def _on_prepare(self, context, request, step):
        def on_prepared(prepared_result):
            result_id = next(self._result_ids)
            self._prepared_results[result_id] = prepared_result
            self._bridge_api.flint_step_succeed(step, result_id)

        self._call(step, lambda: self._prepare(json.loads(request.decode("utf-8"))), on_prepared)

    def _on_run(self, context, result_id, step):
        prepared_result = self._prepared_results.pop(result_id)
        self._call(
            step,
            lambda: self._run(prepared_result, _Output(self._bridge_api, step, False),
                              _Output(self._bridge_api, step, True)),
            lambda value: self._bridge_api.flint_step_succeed(step, 0))

    def _on_discard(self, context, result_id):
        self._prepared_results.pop(result_id, None)

    def _on_release(self, context):
        _released_execution_adapters[:] = [_execution_adapters.pop(context, None)]

    def _call(self, step, invoke, done):
        try:
            result = invoke()
        except BaseException:
            self._fail(step, traceback.format_exc())
            return
        if isinstance(result, Future):
            result.add_done_callback(lambda future: self._settle(step, future, done))
        else:
            done(result)

    def _settle(self, step, future, done):
        try:
            value = future.result()
        except BaseException as error:
            self._fail(step, "".join(traceback.format_exception(type(error), error, error.__traceback__)))
            return
        done(value)

    def _fail(self, step, trace):
        self._bridge_api.flint_step_fail(step, trace.encode("utf-8", "replace"), None)
