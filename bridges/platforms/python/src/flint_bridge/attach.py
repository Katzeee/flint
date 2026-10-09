"""The attach bootstrap's entry into the Python platform."""

import json
import threading
from importlib import import_module


def start(request):
    # The host manager marshals onto the host main thread and waits, so it must
    # not run on the injected thread, which holds the GIL until this returns.
    thread = threading.Thread(target=_attach, args=(json.loads(request),), name="flint-attach", daemon=True)
    thread.start()
    return thread


def _attach(request):
    try:
        manager = import_module(request["module"]).manager
        manager.attach(address=request["address"], port=request["port"], name=request["name"])
    except BaseException as error:
        with open(request["error_path"], "w", encoding="utf-8") as report:
            report.write(str(error) or type(error).__name__)
