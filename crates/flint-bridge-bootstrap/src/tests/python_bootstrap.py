"""Observe generated bootstrap behavior with a controlled host entry and error sink."""
import builtins
from contextlib import contextmanager
import json
import sys
import threading
from types import ModuleType, SimpleNamespace

print("Python interpreter:", sys.executable, file=sys.stderr, flush=True)

entered, resume, reported = threading.Event(), threading.Event(), threading.Event()
observed = {}
workers = []


def attach(**arguments):
    worker = threading.current_thread()
    workers.append(worker)
    observed.update(arguments=arguments, payload=sys.path[0],
                    background=worker is not threading.main_thread(), daemon=worker.daemon)
    entered.set()
    if not resume.wait(3):
        raise TimeoutError("bootstrap did not return while attach was pending")
    raise RuntimeError("attach failed 场景")


@contextmanager
def report(path, mode, encoding):
    observed["report"] = dict(path=path, mode=mode, encoding=encoding)
    yield SimpleNamespace(write=lambda message: observed["report"].update(message=message))
    reported.set()


module = ModuleType("flint_bridge.maya")
module.manager = SimpleNamespace(attach=attach)
sys.modules[module.__name__] = module
builtins.open = report
exec(compile(sys.argv[1], "<bootstrap>", "exec"))
assert entered.wait(3), "bootstrap did not call the host entry"
resume.set()
assert reported.wait(3), "bootstrap did not report the attach failure"
workers[0].join(3)
assert not workers[0].is_alive()
print(json.dumps(observed))
