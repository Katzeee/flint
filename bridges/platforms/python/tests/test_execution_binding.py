from concurrent.futures import Future
import json
import threading

import pytest

from flint_bridge.connection.execution_binding import ExecutionBinding, _bindings
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


def bound(executor, scheduler=None):
    native = Native()
    binding = ExecutionBinding(native, ExecutionCapabilities(executor, scheduler or Scheduler()))
    return native, binding, binding.callbacks


def request(code="code"):
    return json.dumps({"request_id": "r", "code": code}).encode()


def test_synchronous_steps_pass_the_prepared_value_to_run_and_stream_output():
    class Executor:
        def prepare(self, request):
            return request["code"].upper()

        def run(self, prepared, out, err):
            out.write(prepared + "\0🙂")
            err.write("错误")

    native, binding, callbacks = bound(Executor())
    callbacks.prepare(callbacks.context, request(), 1)
    [(_, _, token)] = native.events
    callbacks.run(callbacks.context, token, 2)
    assert native.events[1:] == [
        ("output", 2, "CODE\0🙂", ""), ("output", 2, "", "错误"), ("succeed", 2, 0)]
    assert not binding._prepared


def test_an_executor_without_prepare_runs_the_request():
    received = []

    class Executor:
        def run(self, prepared, out, err):
            received.append(prepared)

    native, binding, callbacks = bound(Executor())
    callbacks.prepare(callbacks.context, request(), 1)
    callbacks.run(callbacks.context, native.events[0][2], 2)
    assert received == [{"request_id": "r", "code": "code"}]


def test_futures_complete_steps_later_from_another_thread():
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
    worker = threading.Thread(target=preparing.set_result, args=("ready",))
    worker.start()
    worker.join()
    callbacks.run(callbacks.context, native.events[0][2], 2)
    assert native.events[-1] == ("output", 2, "ready", "")
    running.set_exception(ValueError("EXPECTED"))
    assert native.events[-1][:2] == ("fail", 2)
    assert "ValueError: EXPECTED" in native.events[-1][2]


def test_failures_carry_tracebacks_and_output_ends_with_the_step():
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


def test_discard_drops_the_prepared_value_and_release_ends_registration():
    native, binding, callbacks = bound(Idle())
    callbacks.prepare(callbacks.context, request(), 1)
    callbacks.discard(callbacks.context, native.events[0][2])
    assert not binding._prepared
    assert _bindings[callbacks.context] is binding
    callbacks.release(callbacks.context)
    assert callbacks.context not in _bindings


class Idle:
    def run(self, prepared, out, err):
        pass


def test_post_hands_the_ticket_to_the_scheduler_and_reports_refusal():
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
    callbacks.release(callbacks.context)
