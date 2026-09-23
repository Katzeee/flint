"""Attach a short Python probe to a fresh, disposable DCC process on Windows.

This experiment uses the private pydevd injector shipped with an installed
debugpy extension. It is not part of flint's supported connection path.
"""

import argparse
import base64
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from uuid import uuid4


PAYLOAD = '''import ctypes, json, os, sys, traceback
from pathlib import Path

cfg = json.loads(CONFIG_JSON)

def save(path, data):
    target = Path(path)
    temporary = target.with_name(target.name + ".tmp")
    temporary.write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
    os.replace(str(temporary), str(target))

save(cfg["native"], {
    "pid": os.getpid(),
    "python": sys.version,
    "native_thread": ctypes.windll.kernel32.GetCurrentThreadId(),
    "bridge_preloaded": "flint_bridge" in sys.modules,
})

def on_host_thread(_=None):
    result = {"pid": os.getpid(), "host": cfg["host"]}
    try:
        import threading
        result["thread"] = threading.get_ident()
        if cfg["host"] == "blender":
            import bpy
            result["main_thread"] = threading.current_thread() is threading.main_thread()
            result["version"] = bpy.app.version_string
            result["objects"] = list(bpy.data.objects.keys())
        else:
            from PySide2 import QtCore, QtWidgets
            app = QtWidgets.QApplication.instance()
            result["main_thread"] = app is not None and QtCore.QThread.currentThread() is app.thread()
            if cfg["host"] == "maya":
                import maya.cmds as cmds
                result["version"] = cmds.about(version=True)
                result["objects"] = cmds.ls(assemblies=True)
            else:
                import pymxs
                result["version"] = str(pymxs.runtime.maxVersion())
                result["objects"] = [str(obj.name) for obj in pymxs.runtime.objects]
        if not result["main_thread"]:
            raise RuntimeError("Probe did not reach the host UI thread")
        if cfg.get("bridge_zip"):
            sys.path.insert(0, cfg["bridge_zip"])
            import flint_bridge
            bridge = flint_bridge.connect(cfg["host"], port=cfg["registry_port"], name="Attach spike")
            if not bridge.wait_until_connected(15):
                raise RuntimeError("Bridge registration did not complete")
            result["instance_id"] = bridge.instance_id
    except BaseException:
        result["error"] = traceback.format_exc()
    save(cfg["main"], result)
    return 0

if cfg["host"] == "maya":
    import maya.utils
    maya.utils.executeDeferred(on_host_thread)
elif cfg["host"] == "max":
    from PySide2 import QtCore, QtWidgets
    class ProbeDispatch(QtCore.QObject):
        requested = QtCore.Signal()
        def __init__(self):
            super().__init__()
            app = QtWidgets.QApplication.instance()
            if app is None:
                raise RuntimeError("Max has no QApplication")
            self.moveToThread(app.thread())
            self.requested.connect(self.run, QtCore.Qt.QueuedConnection)
        @QtCore.Slot()
        def run(self):
            on_host_thread()
    sys._flint_probe_dispatch = ProbeDispatch()
    sys._flint_probe_dispatch.requested.emit()
elif cfg["blender_dispatch"] == "timer":
    # This worker-thread registration is an experiment, not a thread-safety guarantee.
    import bpy
    def timer():
        on_host_thread()
        return None
    bpy.app.timers.register(timer, first_interval=0.0)
else:
    callback_type = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.c_void_p)
    sys._flint_probe_callback = callback_type(on_host_thread)
    ctypes.pythonapi.Py_AddPendingCall.argtypes = [callback_type, ctypes.c_void_p]
    ctypes.pythonapi.Py_AddPendingCall.restype = ctypes.c_int
    if ctypes.pythonapi.Py_AddPendingCall(sys._flint_probe_callback, None) != 0:
        raise RuntimeError("Interpreter pending-call queue is full")
'''


def add_host_arguments(parser):
    parser.add_argument("--host", required=True, choices=("maya", "max", "blender"))
    parser.add_argument("--exe", required=True, type=Path, help="Executable for a new empty host process")
    parser.add_argument("--injector", required=True, type=Path,
                        help="Installed pydevd_attach_to_process directory with helper EXE and DLL files")
    parser.add_argument("--startup-wait", type=float, default=60,
                        help="Seconds to wait after the Python DLL first appears (default: 60)")
    parser.add_argument("--out", type=Path, help="Evidence directory; defaults to a new OS temp directory")
    parser.add_argument("--blender-dispatch", choices=("pending", "timer"), default="pending")


def _loaded_python(pid):
    # The vendored winappdbg module scanner raises a bytes/str error on recent Python.
    names = subprocess.check_output(
        ["powershell.exe", "-NoProfile", "-Command", "(Get-Process -Id %d).Modules.ModuleName" % pid],
        text=True, stderr=subprocess.DEVNULL,
    ).splitlines()
    return any(Path(name).name.lower().startswith("python3") for name in names)


def _wait_for(path, process, seconds):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if path.is_file():
            return json.loads(path.read_text(encoding="utf-8"))
        if process.poll() is not None:
            raise RuntimeError("Host exited; inspect host.log")
        time.sleep(0.1)
    raise TimeoutError("No acknowledgement from target process: " + path.name)


def _bootstrap(path):
    # pydevd's Windows helper accepts a short, single-quote-free UTF-8 string.
    command = "exec(compile(open(%s, encoding=\"utf-8\").read(), %s, \"exec\"))" % (
        json.dumps(str(path)), json.dumps(str(path)))
    encoded = base64.b64encode(command.encode("utf-8")).decode("ascii")
    bootstrap = 'exec(__import__("base64").b64decode("%s"))' % encoded
    if "'" in bootstrap or len(bootstrap.encode("utf-8")) >= 2047:
        raise ValueError("Bootstrap exceeds the injector's shared-memory limit")
    return bootstrap


def run_probe(args, bridge_zip=None, registry_port=None, on_connected=None):
    if sys.platform != "win32":
        raise RuntimeError("This probe uses the Windows pydevd native injector")
    executable = args.exe.resolve(strict=True)
    expected = {"maya": "maya.exe", "max": "3dsmax.exe", "blender": "blender.exe"}[args.host]
    if executable.name.lower() != expected:
        raise ValueError("Executable does not match host: " + executable.name)
    injector = args.injector.resolve(strict=True)
    for name in ("add_code_to_python_process.py", "inject_dll_amd64.exe", "attach_amd64.dll",
                 "run_code_on_dllmain_amd64.dll"):
        if not (injector / name).is_file():
            raise FileNotFoundError(injector / name)
    if args.startup_wait < 0 or args.startup_wait > 300:
        raise ValueError("startup-wait must be between 0 and 300 seconds")
    if bridge_zip is not None and args.host == "blender":
        raise ValueError("The current flint Bridge has no Blender host adapter")
    if (bridge_zip is None) != (registry_port is None):
        raise ValueError("Bridge ZIP and registry port must be supplied together")

    output = args.out or Path(tempfile.gettempdir()) / ("flint-attach-%s-%s" % (args.host, uuid4().hex[:8]))
    output.mkdir(parents=True, exist_ok=bridge_zip is not None)
    for name in ("native.json", "main.json", "payload.py", "host.log", "owned-pid.txt"):
        if (output / name).exists():
            raise FileExistsError(output / name)
    payload = output / "payload.py"
    config = {"host": args.host, "native": str(output / "native.json"), "main": str(output / "main.json"),
              "blender_dispatch": args.blender_dispatch}
    if bridge_zip is not None:
        config.update(bridge_zip=str(bridge_zip), registry_port=registry_port)
    payload.write_text("import json\nCONFIG_JSON = %r\n" % json.dumps(config) + PAYLOAD, encoding="utf-8")
    sys.path.insert(0, str(injector))
    from add_code_to_python_process import run_python_code_windows

    environment = dict(os.environ)
    environment.pop("PYTHONPATH", None)
    environment.pop("PYTHONHOME", None)
    if args.host == "maya":
        environment.update(MAYA_APP_DIR=str(output / "maya-profile"), MAYA_DISABLE_CIP="1", MAYA_DISABLE_CER="1")
    arguments = {"maya": [], "max": ["-q"], "blender": ["--factory-startup", "--disable-autoexec"]}[args.host]
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    with (output / "host.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen([str(executable)] + arguments, env=environment, startupinfo=startup,
                                   stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT)
        try:
            (output / "owned-pid.txt").write_text(str(process.pid), encoding="ascii")
            deadline = time.monotonic() + 120
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError("Host exited during startup; inspect host.log")
                try:
                    loaded = _loaded_python(process.pid)
                except subprocess.CalledProcessError:
                    loaded = False
                if loaded:
                    break
                time.sleep(1)
            else:
                raise TimeoutError("Host Python DLL was not loaded")
            time.sleep(args.startup_wait)
            run_python_code_windows(process.pid, _bootstrap(payload))
            native = _wait_for(output / "native.json", process, 15)
            if native["pid"] != process.pid or native["bridge_preloaded"]:
                raise RuntimeError("Unexpected target identity or existing Bridge")
            main = _wait_for(output / "main.json", process, 60)
            if main.get("error") or not main.get("main_thread") or main["pid"] != process.pid:
                raise RuntimeError("Host callback failed: " + json.dumps(main, ensure_ascii=False))
            if on_connected is not None:
                on_connected(main, process.pid, output)
            print(json.dumps({"host": args.host, "native": native, "main": main,
                              "evidence": str(output)}, ensure_ascii=False, indent=2))
            return main
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=10)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    add_host_arguments(parser)
    run_probe(parser.parse_args())
