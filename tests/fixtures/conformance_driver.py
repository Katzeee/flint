"""Drive the exported Python binding for the runtime conformance scenarios.

Reads one JSON command per line and answers each with one JSON line. Results
are reported as the binding produced them; Rust owns every assertion.
"""
import json
from pathlib import Path
import sys
import time

sys.path.insert(0, sys.argv[1])
from flint_bridge import BridgeBusyError, BridgeCreationError
from flint_bridge.connection import native_core

core = None
held = None
packaged_library = native_core._library_path


def create(command):
    global core
    library = command.get("library")
    native_core._library_path = (lambda: Path(library)) if library else packaged_library
    try:
        created = native_core.NativeCore(command["config"])
    except BridgeCreationError as error:
        return {"error": {"kind": error.kind, "message": str(error)}}
    core = created
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
    global held
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        held = core.poll(100)
        if held is not None:
            return {"request_id": held["request_id"]}
    return {"request_id": None}


def finish(command):
    return {"reported": core.report_execution({
        "kind": "result", "request_id": held["request_id"], "succeeded": True,
        "traceback": None, "error": None,
    })}


def reconnect(command):
    core.reconnect()
    return {"reconnected": True}


def close(command):
    core.stop()
    core.close()
    return {"closed": True}


COMMANDS = {
    "create": create, "apply": apply, "take": take, "finish": finish,
    "reconnect": reconnect, "close": close,
    "status": lambda command: core.status,
}

for line in sys.stdin:
    command = json.loads(line)
    print(json.dumps(COMMANDS[command["op"]](command)), flush=True)
