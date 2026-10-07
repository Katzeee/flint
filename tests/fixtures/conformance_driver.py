"""Drive the exported Python binding for the runtime conformance scenarios.

Reads one JSON command per line and answers each with one JSON line. Results
are reported as the binding produced them; Rust owns every assertion.
"""
import json
from pathlib import Path
import sys
import threading
from queue import Queue

sys.path.insert(0, sys.argv[1])
from flint_bridge import BridgeBusyError, BridgeCreationError
from flint_bridge.connection import native_core
from flint_bridge.execution.capabilities import ExecutionCapabilities
from flint_bridge.execution.scheduling import WorkerThread

core = None
scheduler = None
requests = Queue()
release = threading.Event()


class Executor:
    def run(self, request, out, err):
        requests.put(request)
        if not release.wait(15):
            raise TimeoutError("Test did not release execution")


packaged_library = native_core._library_path


def create(command):
    global core, scheduler
    library = command.get("library")
    native_core._library_path = (lambda: Path(library)) if library else packaged_library
    worker = WorkerThread()
    try:
        created = native_core.NativeCore(command["config"], ExecutionCapabilities(Executor(), worker))
    except BridgeCreationError as error:
        worker.close()
        return {"error": {"kind": error.kind, "message": str(error)}}
    release.clear()
    core, scheduler = created, worker
    return {"created": True}


def apply(command):
    try:
        core.apply_settings(command["settings"])
    except BridgeBusyError:
        return {"rejected": "busy"}
    except ValueError:
        return {"rejected": "invalid_settings"}
    return {"applied": True}


def take(command):
    return {"request_id": requests.get(timeout=10)["request_id"]}


def finish(command):
    release.set()
    return {"reported": True}


def reconnect(command):
    core.reconnect()
    return {"reconnected": True}


def close(command):
    if not core.stop():
        return {"closed": False}
    core.destroy()
    scheduler.close()
    return {"closed": True}


COMMANDS = {
    "create": create, "apply": apply, "take": take, "finish": finish,
    "reconnect": reconnect, "close": close,
    "status": lambda command: core.status,
}

for line in sys.stdin:
    command = json.loads(line)
    print(json.dumps(COMMANDS[command["op"]](command)), flush=True)
