"""Drive the exported Python binding for the runtime conformance scenarios.

Reads one JSON command per line and answers each with one JSON line. Results
are reported as the binding produced them; Rust owns every assertion.
"""
import json
import sys
import threading
from queue import Queue

sys.path.insert(0, sys.argv[1])
from flint_bridge import BridgeBusyError
from flint_bridge.connection import native_core
from flint_bridge.execution.capabilities import ExecutionCapabilities
from flint_bridge.execution.executor import CodeExecutor
from flint_bridge.execution.scheduling import WorkerThread

core = None
scheduler = None
requests = Queue()
release = threading.Event()


class Executor(CodeExecutor):
    def prepare(self, request):
        return request, super().prepare(request)

    def run(self, prepared, out, err):
        request, code = prepared
        requests.put(request)
        if not release.wait(15):
            raise TimeoutError("Test did not release execution")
        super().run(code, out, err)


def create(command):
    global core, scheduler
    worker = WorkerThread()
    try:
        created = native_core.NativeCore(command["config"], ExecutionCapabilities(Executor(), worker))
    except BaseException:
        worker.close()
        raise
    release.clear()
    core, scheduler = created, worker
    return {"created": True}


def apply(command):
    try:
        core.apply_settings(command["settings"])
    except BridgeBusyError:
        return {"rejected": "busy"}
    return {"applied": True}


def take(command):
    return {"request_id": requests.get(timeout=10)["request_id"]}


def finish(command):
    release.set()
    return {"reported": True}


def close(command):
    if not core.stop():
        return {"closed": False}
    core.destroy()
    scheduler.close()
    return {"closed": True}


COMMANDS = {
    "create": create, "apply": apply, "take": take, "finish": finish,
    "close": close,
    "status": lambda command: core.status,
}

for line in sys.stdin:
    command = json.loads(line)
    print(json.dumps(COMMANDS[command["op"]](command)), flush=True)
