"""Exercise the Rust backend through a freshly attached Maya or Max process."""

import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import time
from uuid import uuid4

from probe import add_host_arguments, run_probe


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def run_spike(args):
    if args.host == "blender":
        raise ValueError("Blender has no Bridge adapter yet; use probe.py")
    flint = args.flint_exe.resolve(strict=True)
    if flint.name.lower() != "flint.exe":
        raise ValueError("Expected flint.exe")
    output = args.out or Path(tempfile.gettempdir()) / ("flint-bridge-spike-%s-%s" % (args.host, uuid4().hex[:8]))
    output.mkdir(parents=True, exist_ok=False)
    args.out = output
    port, registry_port = free_port(), free_port()
    while port == registry_port:
        registry_port = free_port()
    environment = dict(os.environ, FLINT_STATE_DIR=str(output / "state"))
    options = ["--host", "127.0.0.1", "--port", str(port), "--registry-host", "127.0.0.1",
               "--registry-port", str(registry_port), "--no-tray", "--json"]

    def command(name, *arguments, expected=0):
        result = subprocess.run([str(flint), name] + options + list(arguments), env=environment,
                                cwd=output, capture_output=True, text=True, encoding="utf-8", timeout=45)
        if result.returncode != expected:
            raise RuntimeError("%s failed: %s %s" % (name, result.stdout, result.stderr))
        return json.loads(result.stdout)

    bridge_zip = output / "flint-python.zip"
    subprocess.run([str(flint), "bridge", "export", "python", "--output", str(bridge_zip)],
                   cwd=output, check=True, capture_output=True, timeout=20)
    command("start")
    try:
        def connected(main, pid, evidence):
            instance = main["instance_id"]
            instances = command("instances", "--type", args.host)["instances"]
            if len(instances) != 1 or instances[0]["instance_id"] != instance or not instances[0]["execution_ready"]:
                raise AssertionError("Injected Bridge is not ready for execution")
            if instances[0]["pid"] != pid:
                raise AssertionError("Backend registered another process")
            workflow = command("workflow", "--name", "Attach spike")["workflow_id"]
            thread_check = ("from PySide2 import QtCore, QtWidgets\n"
                            "assert QtCore.QThread.currentThread() is QtWidgets.QApplication.instance().thread()\n")
            if args.host == "maya":
                scene = ("import maya.cmds as cmds\n"
                         "obj = cmds.createNode('transform', name='FlintAttachSpike')\n"
                         "try:\n    assert cmds.objExists(obj)\nfinally:\n    cmds.delete(obj)\n"
                         "assert not cmds.objExists(obj)\n")
            else:
                scene = ("import pymxs\nrt = pymxs.runtime\n"
                         "obj = rt.Point(name='FlintAttachSpike')\n"
                         "try:\n    assert rt.isValidNode(obj)\nfinally:\n    rt.delete(obj)\n"
                         "assert rt.getNodeByName('FlintAttachSpike') is None\n")
            source = thread_check + scene + "import os\nprint('ATTACH_OK', os.getpid())"
            execution = command("exec", "--instance-id", instance, "--workflow-id", workflow,
                                "--code", source)
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                detail = command("execution", "--workflow-id", workflow,
                                 "--execution-id", execution["execution_id"], "--view", "full")
                if detail["status"] in ("succeeded", "failed"):
                    break
                time.sleep(0.25)
            else:
                raise TimeoutError("Host execution did not finish")
            (evidence / "execution.json").write_text(json.dumps(detail, indent=2), encoding="utf-8")
            if detail["status"] != "succeeded" or "ATTACH_OK %d" % pid not in detail["stdout"]:
                raise AssertionError("Injected Bridge could not execute on the host thread: " + str(detail))

        run_probe(args, bridge_zip=bridge_zip, registry_port=registry_port, on_connected=connected)
    finally:
        command("stop")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    add_host_arguments(parser)
    parser.add_argument("--flint-exe", required=True, type=Path, help="A built flint.exe")
    run_spike(parser.parse_args())
