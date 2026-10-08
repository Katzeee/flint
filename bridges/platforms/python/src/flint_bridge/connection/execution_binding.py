"""The execution binding layout and native step signatures."""
import ctypes as c

POST = c.CFUNCTYPE(c.c_bool, c.c_size_t, c.c_void_p)
PREPARE = c.CFUNCTYPE(None, c.c_size_t, c.c_char_p, c.c_void_p)
RUN = c.CFUNCTYPE(None, c.c_size_t, c.c_size_t, c.c_void_p)
DISCARD = c.CFUNCTYPE(None, c.c_size_t, c.c_size_t)
RELEASE = c.CFUNCTYPE(None, c.c_size_t)


class ExecutionBinding(c.Structure):
    _fields_ = [("context", c.c_size_t), ("post", POST), ("prepare", PREPARE), ("run", RUN),
                ("discard", DISCARD), ("release", RELEASE)]


def bind(bridge_api):
    for name, arguments, result in (
        ("step_run", [c.c_void_p], None),
        ("step_output", [c.c_void_p, c.c_char_p, c.c_size_t, c.c_char_p, c.c_size_t], c.c_bool),
        ("step_succeed", [c.c_void_p, c.c_size_t], None),
        ("step_fail", [c.c_void_p, c.c_char_p, c.c_char_p], None),
    ):
        function = getattr(bridge_api, "flint_" + name)
        function.argtypes, function.restype = arguments, result
