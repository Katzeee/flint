from concurrent.futures import Future
import ctypes
import gc
import json
import threading
import weakref

import pytest

from flint_bridge.connection.execution_binding import ExecutionBinding, RELEASE
from flint_bridge.execution.capabilities import ExecutionCapabilities


class Native:
    """Records what the binding reports to the core for each step pointer."""

    def __init__(self):
        self.events = []
        self.tickets = []

    def flint_ticket_run(self, ticket):
        self.tickets.append(ticket)

    def flint_step_output(self, step, out, out_len, err, err_len):
        self.events.append(("output", step, out[:out_len].decode(), err[:err_len].decode()))
        return True

    def flint_step_succeed(self, step, prepared):
        self.events.append(("succeed", step, prepared))

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
        native = Native()
        binding = ExecutionBinding(native, ExecutionCapabilities(executor, scheduler or Scheduler()))
        # The native owner holds an address, not a Python callback that would pin the binding itself.
        release = RELEASE(ctypes.cast(binding.callbacks.release, ctypes.c_void_p).value)
        registrations[binding.callbacks.context] = release, weakref.ref(binding)
        return native, binding, binding.callbacks

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


def test_synchronous_steps_pass_the_prepared_value_to_run_and_stream_output(bound):
    values = []

    class Prepared:
        def __init__(self, code):
            self.code = code

    class Executor:
        def prepare(self, request):
            value = Prepared(request["code"].upper())
            values.append(weakref.ref(value))
            return value

        def run(self, prepared, out, err):
            out.write(prepared.code + "\0🙂")
            err.write("错误")

    native, binding, callbacks = bound(Executor())
    callbacks.prepare(callbacks.context, request(), 1)
    gc.collect()
    assert values[0]() is not None
    [(_, _, token)] = native.events
    callbacks.run(callbacks.context, token, 2)
    events = native.events[1:]
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

        def run(self, prepared, out, err):
            out.write(prepared)
            return running

    native, binding, callbacks = bound(Executor())
    callbacks.prepare(callbacks.context, request(), 1)
    assert native.events == []
    worker = threading.Thread(target=preparing.set_result, args=("ready",), daemon=True)
    worker.start()
    worker.join(3)
    assert not worker.is_alive()
    callbacks.run(callbacks.context, native.events[0][2], 2)
    assert native.events[-1] == ("output", 2, "ready", "")
    running.set_exception(ValueError("EXPECTED"))
    assert native.events[-1][:2] == ("fail", 2)
    assert "ValueError: EXPECTED" in native.events[-1][2]


def test_failures_carry_tracebacks_and_output_ends_with_the_step(bound):
    streams = []

    class Executor:
        def prepare(self, request):
            if request["code"] == "bad":
                raise SyntaxError("EXPECTED")
            return request

        def run(self, prepared, out, err):
            streams.append(out)
            raise SystemExit(0)

    native, binding, callbacks = bound(Executor())
    callbacks.prepare(callbacks.context, request("bad"), 1)
    assert native.events[0][:2] == ("fail", 1) and "SyntaxError: EXPECTED" in native.events[0][2]
    callbacks.prepare(callbacks.context, request(), 2)
    callbacks.run(callbacks.context, native.events[1][2], 3)
    assert native.events[2][:2] == ("fail", 3) and "SystemExit" in native.events[2][2]
    streams[0].write("late")
    assert len(native.events) == 3
    with pytest.raises(TypeError):
        streams[0].write(b"bytes")


def test_discard_drops_the_prepared_value_and_release_ends_registration(bound):
    values = []

    class Prepared:
        pass

    class Executor(Idle):
        def prepare(self, request):
            value = Prepared()
            values.append(weakref.ref(value))
            return value

    native, binding, callbacks = bound(Executor())
    callbacks.prepare(callbacks.context, request(), 1)
    gc.collect()
    assert values[0]() is not None
    callbacks.discard(callbacks.context, native.events[0][2])
    gc.collect()
    assert values[0]() is None
    registered = weakref.ref(binding)
    context = callbacks.context
    del callbacks, binding
    gc.collect()
    assert registered() is not None
    bound.release(context)

    # Releasing a later registration advances past the first release callback's lifetime.
    _, replacement, _ = bound(Idle())
    bound.release(replacement.callbacks.context)
    gc.collect()
    assert registered() is None


class Idle:
    def run(self, prepared, out, err):
        pass


def test_post_hands_the_ticket_to_the_scheduler_and_reports_refusal(bound):
    scheduler = Scheduler()
    native, binding, callbacks = bound(Idle(), scheduler)
    assert callbacks.post(callbacks.context, 41)
    scheduler.posted[0]()
    assert native.tickets == [41]

    class Closed(Scheduler):
        def post(self, callback):
            raise RuntimeError("closed")

    native, binding, callbacks = bound(Idle(), Closed())
    assert not callbacks.post(callbacks.context, 42)
    bound.release(callbacks.context)
