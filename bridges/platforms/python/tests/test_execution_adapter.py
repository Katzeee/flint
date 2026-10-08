from concurrent.futures import Future
import ctypes
import gc
import json
import threading
import weakref

import pytest

from flint_bridge.connection.execution_adapter import ExecutionAdapter
from flint_bridge.connection.execution_binding import RELEASE
from flint_bridge.execution.capabilities import ExecutionCapabilities


class BridgeApi:
    """Records what the execution adapter reports to the core for each step."""

    def __init__(self):
        self.events = []
        self.runs = []

    def flint_step_run(self, step):
        self.runs.append(step)

    def flint_step_output(self, step, out, out_len, err, err_len):
        self.events.append(("output", step, out[:out_len].decode(), err[:err_len].decode()))
        return True

    def flint_step_succeed(self, step, result_id):
        self.events.append(("succeed", step, result_id))

    def flint_step_fail(self, step, trace, error):
        self.events.append(("fail", step, trace.decode(), error))


class Scheduler:
    def __init__(self):
        self.posted = []

    def post(self, callback):
        self.posted.append(callback)

    def close(self):
        pass


@pytest.fixture
def bound():
    registrations = {}

    def create(executor, scheduler=None):
        bridge_api = BridgeApi()
        execution_adapter = ExecutionAdapter(bridge_api, ExecutionCapabilities(executor, scheduler or Scheduler()))
        # The native owner holds an address, not a Python callback that would pin the execution adapter itself.
        release = RELEASE(ctypes.cast(execution_adapter.execution_binding.release, ctypes.c_void_p).value)
        registrations[execution_adapter.execution_binding.context] = release, weakref.ref(execution_adapter)
        return bridge_api, execution_adapter, execution_adapter.execution_binding

    def release(context):
        callback, reference = registrations.pop(context)
        if reference() is not None:
            callback(context)

    create.release = release
    yield create
    for context in list(registrations):
        release(context)


def request(code="code"):
    return json.dumps({"request_id": "r", "code": code}).encode()


def test_synchronous_steps_pass_the_prepared_result_to_run_and_stream_output(bound):
    values = []

    class PreparedResult:
        def __init__(self, code):
            self.code = code

    class Executor:
        def prepare(self, request):
            value = PreparedResult(request["code"].upper())
            values.append(weakref.ref(value))
            return value

        def run(self, prepared_result, out, err):
            out.write(prepared_result.code + "\0🙂")
            err.write("错误")

    bridge_api, execution_adapter, execution_binding = bound(Executor())
    execution_binding.prepare(execution_binding.context, request(), 1)
    gc.collect()
    assert values[0]() is not None
    [(_, _, result_id)] = bridge_api.events
    execution_binding.run(execution_binding.context, result_id, 2)
    events = bridge_api.events[1:]
    output = [event for event in events if event[0] == "output"]
    assert all(event[1] == 2 for event in output)
    assert "".join(event[2] for event in output) == "CODE\0🙂"
    assert "".join(event[3] for event in output) == "错误"
    assert [event for event in events if event[0] != "output"] == [("succeed", 2, 0)]
    assert events[-1] == ("succeed", 2, 0)
    gc.collect()
    assert values[0]() is None


def test_futures_complete_steps_later_from_another_thread(bound):
    preparing, running = Future(), Future()

    class Executor:
        def prepare(self, request):
            return preparing

        def run(self, prepared_result, out, err):
            out.write(prepared_result)
            return running

    bridge_api, execution_adapter, execution_binding = bound(Executor())
    execution_binding.prepare(execution_binding.context, request(), 1)
    assert bridge_api.events == []
    worker = threading.Thread(target=preparing.set_result, args=("ready",), daemon=True)
    worker.start()
    worker.join(3)
    assert not worker.is_alive()
    execution_binding.run(execution_binding.context, bridge_api.events[0][2], 2)
    assert bridge_api.events[-1] == ("output", 2, "ready", "")
    running.set_exception(ValueError("EXPECTED"))
    assert bridge_api.events[-1][:2] == ("fail", 2)
    assert "ValueError: EXPECTED" in bridge_api.events[-1][2]


def test_failures_carry_tracebacks(bound):
    streams = []

    class Executor:
        def prepare(self, request):
            if request["code"] == "bad":
                raise SyntaxError("EXPECTED")
            return request

        def run(self, prepared_result, out, err):
            streams.append(out)
            raise SystemExit(0)

    bridge_api, execution_adapter, execution_binding = bound(Executor())
    execution_binding.prepare(execution_binding.context, request("bad"), 1)
    assert bridge_api.events[0][:2] == ("fail", 1) and "SyntaxError: EXPECTED" in bridge_api.events[0][2]
    execution_binding.prepare(execution_binding.context, request(), 2)
    execution_binding.run(execution_binding.context, bridge_api.events[1][2], 3)
    assert bridge_api.events[2][:2] == ("fail", 3) and "SystemExit" in bridge_api.events[2][2]
    with pytest.raises(TypeError):
        streams[0].write(b"bytes")


def test_discard_drops_the_prepared_result_and_release_ends_registration(bound):
    values = []

    class PreparedResult:
        pass

    class Executor(Idle):
        def prepare(self, request):
            value = PreparedResult()
            values.append(weakref.ref(value))
            return value

    bridge_api, execution_adapter, execution_binding = bound(Executor())
    execution_binding.prepare(execution_binding.context, request(), 1)
    gc.collect()
    assert values[0]() is not None
    execution_binding.discard(execution_binding.context, bridge_api.events[0][2])
    gc.collect()
    assert values[0]() is None
    registered = weakref.ref(execution_adapter)
    context = execution_binding.context
    del execution_binding, execution_adapter
    gc.collect()
    assert registered() is not None
    bound.release(context)

    # Releasing a later registration advances past the first release callback's lifetime.
    _, replacement, _ = bound(Idle())
    bound.release(replacement.execution_binding.context)
    gc.collect()
    assert registered() is None


class Idle:
    def run(self, prepared_result, out, err):
        pass


def test_post_hands_the_step_to_the_scheduler_and_reports_refusal(bound):
    scheduler = Scheduler()
    bridge_api, execution_adapter, execution_binding = bound(Idle(), scheduler)
    assert execution_binding.post(execution_binding.context, 41)
    scheduler.posted[0]()
    assert bridge_api.runs == [41]

    class Closed(Scheduler):
        def post(self, callback):
            raise RuntimeError("closed")

    bridge_api, execution_adapter, execution_binding = bound(Idle(), Closed())
    assert not execution_binding.post(execution_binding.context, 42)
    bound.release(execution_binding.context)
