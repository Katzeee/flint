"""The attach bootstrap's entry into the Python platform."""

import json
import os
import threading
from importlib import import_module


def start(plan, report_path):
    # The host manager marshals onto the host main thread and waits, so it must
    # not run on the injected thread, which holds the GIL until this returns.
    thread = threading.Thread(target=_attach, args=(json.loads(plan), report_path), name="flint-attach", daemon=True)
    thread.start()
    return thread


def _attach(plan, report_path):
    try:
        manager = import_module(plan["module"]).manager
        manager.attach(**plan["settings"])
        outcome = ""
    except BaseException as error:
        outcome = str(error) or type(error).__name__
    # Empty reports success, so a reader must never see the file before its content.
    staged = os.path.splitext(report_path)[0] + ".staged"
    with open(staged, "w", encoding="utf-8") as report:
        report.write(outcome)
    os.replace(staged, report_path)
