"""Drive the exported manager with controlled host capabilities; Rust judges it."""
import json
from pathlib import Path
from queue import Queue, Empty
import sys
import threading

sys.path.insert(0, sys.argv[1])
from flint_bridge import BridgeManager, BridgeBusyError, BridgeCreationError, BridgeStoppedError
from flint_bridge.connection import native_core
from flint_bridge.execution.models import InstanceExecResult, InstanceExecStatus
from flint_bridge.execution.runner import CodeRunner
from flint_bridge.execution.strategies.direct import DirectExecutionStrategy

probe = {"created": 0, "released": 0, "finished": 0, "factory_thread": None, "dispatch_thread": None}
started, release = threading.Event(), threading.Event()
queued = Queue()
hold_dispatch = False
attach_worker = None
attach_result = None
packaged_library = native_core._library_path


class Strategy(DirectExecutionStrategy):
    def _close(self):
        probe["released"] += 1


class Executor:
    def execute(self, execution_id, code, out, err, filename):
        started.set()
        if not release.wait(25):
            raise TimeoutError("Test did not release the host execution")
        probe["finished"] += 1
        return InstanceExecResult(execution_id, InstanceExecStatus.SUCCEEDED)


def create_runner():
    probe["created"] += 1
    probe["factory_thread"] = threading.get_ident()
    return CodeRunner(Executor(), Strategy())


def run_dispatched(callback):
    probe["dispatch_thread"] = threading.get_ident()
    callback()


def dispatch(callback):
    if hold_dispatch:
        queued.put(callback)
    else:
        threading.Thread(target=run_dispatched, args=(callback,), daemon=True).start()


manager = BridgeManager("standalone_python", create_runner, dispatch)


def call(command):
    global hold_dispatch, attach_worker, attach_result
    operation = command["op"]
    try:
        if operation == "create":
            config = command["config"]
            library = command.get("library")
            native_core._library_path = (lambda: Path(library)) if library else packaged_library
            try:
                manager.connect(address=config["address"], port=config["port"], name=config["name"])
            finally:
                native_core._library_path = packaged_library
            return {"created": True}
        if operation == "claim":
            core = native_core.NativeCore(command["config"])
            core.close()
            return {"created": True}
        if operation == "apply":
            manager.configure(**command["settings"])
            return {"applied": True}
        if operation == "attach":
            return {"instance_id": manager.attach(**command["settings"], timeout=command.get("timeout_ms", 20000) / 1000)}
        if operation == "status":
            bridge = manager.current()
            return bridge.status if bridge else None
        if operation == "reconnect":
            manager.reconnect()
            return {"reconnected": True}
        if operation == "close":
            return {"closed": manager.disconnect()}
        if operation == "probe":
            return dict(probe)
        if operation == "wait_started":
            return {"started": started.wait(5)}
        if operation == "release":
            release.set()
            return {"released": True}
        if operation == "attach_begin":
            hold_dispatch = True
            attach_result = None

            def attach():
                global attach_result
                attach_result = call(dict(command, op="attach"))

            attach_worker = threading.Thread(target=attach, daemon=True)
            attach_worker.start()
            # A callback in this queue proves that production attach dispatched it.
            callback = queued.get(timeout=5)
            queued.put(callback)
            return {"queued": True}
        if operation == "attach_result":
            attach_worker.join(5)
            if attach_worker.is_alive():
                raise TimeoutError("Attach did not return")
            return attach_result
        if operation == "drain":
            hold_dispatch = False
            while True:
                try:
                    callback = queued.get_nowait()
                except Empty:
                    break
                run_dispatched(callback)
            return {"drained": True}
        raise ValueError("Unknown operation: " + operation)
    except BridgeStoppedError:
        return {"rejected": "stopped"}
    except BridgeBusyError:
        return {"rejected": "busy"}
    except BridgeCreationError as error:
        return {"error": {"kind": error.kind, "message": str(error)}}
    except ValueError:
        return {"rejected": "invalid_settings"}
    except Exception as error:
        return {"error": {"type": type(error).__name__, "message": str(error)}}


try:
    for line in sys.stdin:
        print(json.dumps(call(json.loads(line))), flush=True)
finally:
    release.set()
    manager.disconnect()
