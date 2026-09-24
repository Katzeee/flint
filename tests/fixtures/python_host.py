"""A disposable Python host; Rust owns orchestration and assertions."""
import json
import os
from pathlib import Path
import sys
import time
import traceback

config = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
directory = Path(config["directory"])
report = {"pid": os.getpid(), "version": "%s.%s" % sys.version_info[:2]}
try:
    sys.path.insert(0, config["bundle"])
    from flint_bridge import connect, disconnect
    bridge = connect("python", port=config["port"])
    if not bridge.wait_until_connected(10):
        raise RuntimeError("Both channels did not become ready")
    report["instance_id"] = bridge.instance_id
    report["reused"] = connect("python", port=config["port"]) is bridge
except BaseException:
    report["error"] = traceback.format_exc()
ready = directory / "ready.json"
ready.write_text(json.dumps(report), encoding="utf-8")
if "error" not in report:
    await_idle_after_drop = False
    while not (directory / "stop-host").exists():
        ping = directory / "ping-host"
        if ping.exists():
            ping.unlink()
            (directory / "host-alive").write_text(str(os.getpid()), encoding="utf-8")
        trigger = directory / "drop-execution"
        if trigger.exists():
            trigger.unlink()
            # Transport fault injection stays inside this disposable fixture.
            bridge._force_reconnect()
            await_idle_after_drop = True
        if await_idle_after_drop and not bridge.busy:
            (directory / "bridge-idle-after-drop").write_text("idle", encoding="utf-8")
            await_idle_after_drop = False
        time.sleep(0.05)
    disconnect()
