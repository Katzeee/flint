"""Translate the core's host callbacks into Python calls and completions."""
from concurrent.futures import Future
import ctypes as c
import itertools
import json
import threading
import traceback

POST = c.CFUNCTYPE(c.c_bool, c.c_size_t, c.c_void_p)
PREPARE = c.CFUNCTYPE(None, c.c_size_t, c.c_char_p, c.c_void_p)
RUN = c.CFUNCTYPE(None, c.c_size_t, c.c_size_t, c.c_void_p)
DISCARD = c.CFUNCTYPE(None, c.c_size_t, c.c_size_t)
RELEASE = c.CFUNCTYPE(None, c.c_size_t)

# The core may call back until it releases the registration. A released binding
# outlives its own release call, whose callback object must not be freed mid-call.
_bindings = {}
_released = []


class Host(c.Structure):
    _fields_ = [("context", c.c_size_t), ("post", POST), ("prepare", PREPARE), ("run", RUN),
                ("discard", DISCARD), ("release", RELEASE)]


def bind(library):
    for name, arguments, result in (
        ("ticket_run", [c.c_void_p], None),
        ("step_output", [c.c_void_p, c.c_char_p, c.c_size_t, c.c_char_p, c.c_size_t], c.c_bool),
        ("step_succeed", [c.c_void_p, c.c_size_t], None),
        ("step_fail", [c.c_void_p, c.c_char_p, c.c_char_p], None),
    ):
        function = getattr(library, "flint_" + name)
        function.argtypes, function.restype = arguments, result


class _Step:
    """One completion owed to the core; output stops when it completes."""

    def __init__(self, library, pointer):
        self._library, self._pointer = library, pointer
        self._lock = threading.Lock()

    def write(self, text, stderr):
        data = text.encode("utf-8", "replace")
        out, err = (b"", data) if stderr else (data, b"")
        with self._lock:
            if self._pointer is not None:
                self._library.flint_step_output(self._pointer, out, len(out), err, len(err))

    def succeed(self, prepared=0):
        pointer = self._take()
        if pointer is not None:
            self._library.flint_step_succeed(pointer, prepared)

    def fail(self, trace):
        pointer = self._take()
        if pointer is not None:
            self._library.flint_step_fail(pointer, trace.encode("utf-8", "replace"), None)

    def _take(self):
        with self._lock:
            pointer, self._pointer = self._pointer, None
        return pointer


class _Output:
    def __init__(self, step, stderr):
        self._step, self._stderr = step, stderr

    def write(self, text):
        if not isinstance(text, str):
            raise TypeError("write() argument must be str, not " + type(text).__name__)
        self._step.write(text, self._stderr)
        return len(text)

    def flush(self):
        pass

    def isatty(self):
        return False


def _identity(request):
    return request


class ExecutionBinding:
    """Holds Python references for the core; a value or Future completes each step."""

    def __init__(self, library, capabilities):
        self._library = library
        self._prepare = getattr(capabilities.executor, "prepare", _identity)
        self._run = capabilities.executor.run
        self._scheduler = capabilities.scheduler
        self._prepared = {}
        self._tokens = itertools.count(1)
        self.callbacks = Host(id(self), POST(self._on_post), PREPARE(self._on_prepare), RUN(self._on_run),
                              DISCARD(self._on_discard), RELEASE(self._on_release))
        _bindings[id(self)] = self

    def _on_post(self, context, ticket):
        try:
            self._scheduler.post(lambda: self._library.flint_ticket_run(ticket))
            return True
        except BaseException:
            traceback.print_exc()
            return False

    def _on_prepare(self, context, request, pointer):
        step = _Step(self._library, pointer)

        def prepared(value):
            token = next(self._tokens)
            self._prepared[token] = value
            step.succeed(token)

        self._call(step, lambda: self._prepare(json.loads(request.decode("utf-8"))), prepared)

    def _on_run(self, context, token, pointer):
        step = _Step(self._library, pointer)
        prepared = self._prepared.pop(token)
        self._call(step, lambda: self._run(prepared, _Output(step, False), _Output(step, True)),
                   lambda value: step.succeed())

    def _on_discard(self, context, token):
        self._prepared.pop(token, None)

    def _on_release(self, context):
        _released[:] = [_bindings.pop(context, None)]

    @staticmethod
    def _call(step, invoke, done):
        try:
            result = invoke()
        except BaseException:
            step.fail(traceback.format_exc())
            return
        if isinstance(result, Future):
            result.add_done_callback(lambda future: ExecutionBinding._settle(step, future, done))
        else:
            done(result)

    @staticmethod
    def _settle(step, future, done):
        try:
            value = future.result()
        except BaseException as error:
            step.fail("".join(traceback.format_exception(type(error), error, error.__traceback__)))
            return
        done(value)
