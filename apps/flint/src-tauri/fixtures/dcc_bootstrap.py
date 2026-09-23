"""Runs inside a fresh DCC process. Rust owns the test lifecycle."""
import json
import os
from pathlib import Path
import sys
import traceback

report = {"pid": os.getpid(), "python": sys.version, "host": CONFIG["host"]}
try:
    sys.path.insert(0, CONFIG["bundle"])
    from PySide2 import QtCore, QtWidgets
    from flint_bridge import connect
    report["main_thread"] = QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()
    if not report["main_thread"]:
        raise RuntimeError("The host must bootstrap on its UI thread")
    if CONFIG["host"] == "maya":
        import maya.cmds as cmds
        report["version"] = cmds.about(version=True)
        report["scene"] = cmds.file(query=True, sceneName=True)
    else:
        import pymxs
        report["version"] = str(pymxs.runtime.maxVersion())
        report["scene"] = str(pymxs.runtime.maxFileName)
    bridge = connect(host=CONFIG["host"], name="Rust integration test", port=CONFIG["port"])
    if not bridge.wait_until_connected(15):
        raise RuntimeError("Both bridge channels did not connect")
    report["instance_id"] = bridge.instance_id
except BaseException:
    report["error"] = traceback.format_exc()
Path(CONFIG["ready"]).write_text(json.dumps(report), encoding="utf-8")
