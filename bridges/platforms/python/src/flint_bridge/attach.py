"""The attach bootstrap's entry into the Python platform."""

import json
import threading
from importlib import import_module


def start(plan, error_path):
    # The host manager marshals onto the host main thread and waits, so it must
    # not run on the injected thread, which holds the GIL until this returns.
    thread = threading.Thread(target=_attach, args=(json.loads(plan), error_path), name="flint-attach", daemon=True)
    thread.start()
    return thread


def _attach(plan, error_path):
    try:
        manager = import_module(plan["module"]).manager
        manager.attach(**plan["settings"])
    except BaseException as error:
        with open(error_path, "w", encoding="utf-8") as report:
            report.write(str(error) or type(error).__name__)
