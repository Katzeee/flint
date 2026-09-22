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
    bridge = connect("python", port=config["port"], heartbeat_interval=0.25)
    if not bridge.wait_until_connected(10):
        raise RuntimeError("Both channels did not become ready")
    report["instance_id"] = bridge.instance_id
    report["reused"] = connect("python", port=config["port"]) is bridge
except BaseException:
    report["error"] = traceback.format_exc()
ready = directory / "ready.json"
ready.write_text(json.dumps(report), encoding="utf-8")
if "error" not in report:
    while not (directory / "stop-host").exists():
        trigger = directory / "drop-execution"
        if trigger.exists():
            trigger.unlink()
            # Transport fault injection stays inside this disposable fixture.
            bridge._loop.call_soon_threadsafe(bridge._writers[1].close)
        time.sleep(0.05)
    disconnect()
