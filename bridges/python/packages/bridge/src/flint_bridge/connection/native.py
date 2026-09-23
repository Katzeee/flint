"""C ABI access to the shared host-side connection core."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import pkgutil
import sys
import tempfile


def _library_name():
    if sys.platform == "win32":
        return "flint_bridge_core.dll"
    if sys.platform == "darwin":
        return "libflint_bridge_core.dylib"
    return "libflint_bridge_core.so"


def _library_path():
    configured = os.environ.get("FLINT_BRIDGE_CORE_LIBRARY")
    if configured:
        return Path(configured)
    data = pkgutil.get_data("flint_bridge", "native/" + _library_name())
    if data is None:
        raise RuntimeError("Bridge package has no native connection core")
    digest = hashlib.sha256(data).hexdigest()[:20]
    folder = Path(tempfile.gettempdir()) / "flint-bridge" / digest
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / _library_name()
    if target.exists():
        if target.read_bytes() != data:
            raise RuntimeError("Extracted Bridge core does not match the package")
    else:
        temporary = folder / (target.name + "." + str(os.getpid()) + ".tmp")
        temporary.write_bytes(data)
        try:
            os.replace(str(temporary), str(target))
        except OSError:
            if not target.exists() or target.read_bytes() != data:
                raise
            if temporary.exists():
                temporary.unlink()
    return target


def _load():
    library = ctypes.CDLL(str(_library_path()))
    library.flint_bridge_abi_version.restype = ctypes.c_uint32
    if library.flint_bridge_abi_version() != 1:
        raise RuntimeError("Unsupported native Bridge ABI")
    library.flint_bridge_create.argtypes = [ctypes.c_char_p]
    library.flint_bridge_create.restype = ctypes.c_void_p
    library.flint_bridge_poll.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    library.flint_bridge_poll.restype = ctypes.c_void_p
    library.flint_bridge_submit.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
    library.flint_bridge_submit.restype = ctypes.c_bool
    library.flint_bridge_connected.argtypes = [ctypes.c_void_p]
    library.flint_bridge_connected.restype = ctypes.c_bool
    library.flint_bridge_busy.argtypes = [ctypes.c_void_p]
    library.flint_bridge_busy.restype = ctypes.c_bool
    library.flint_bridge_instance_id.argtypes = [ctypes.c_void_p]
    library.flint_bridge_instance_id.restype = ctypes.c_void_p
    library.flint_bridge_reconnect.argtypes = [ctypes.c_void_p]
    library.flint_bridge_stop.argtypes = [ctypes.c_void_p]
    library.flint_bridge_destroy.argtypes = [ctypes.c_void_p]
    library.flint_bridge_string_free.argtypes = [ctypes.c_void_p]
    return library


class NativeCore:
    def __init__(self, config):
        self._library = _load()
        encoded = json.dumps(config, ensure_ascii=False).encode("utf-8")
        self._handle = self._library.flint_bridge_create(encoded)
        if not self._handle:
            raise RuntimeError("Cannot start native Bridge core")

    def _string(self, pointer):
        if not pointer:
            return None
        try:
            return ctypes.string_at(pointer).decode("utf-8")
        finally:
            self._library.flint_bridge_string_free(pointer)

    def poll(self, timeout_ms):
        event = self._string(self._library.flint_bridge_poll(self._handle, timeout_ms))
        return json.loads(event) if event is not None else None

    def submit(self, command):
        encoded = json.dumps(command, ensure_ascii=False).encode("utf-8")
        return self._library.flint_bridge_submit(self._handle, encoded)

    @property
    def connected(self):
        return bool(self._library.flint_bridge_connected(self._handle))

    @property
    def busy(self):
        return bool(self._library.flint_bridge_busy(self._handle))

    @property
    def instance_id(self):
        return self._string(self._library.flint_bridge_instance_id(self._handle)) or None

    def reconnect(self):
        self._library.flint_bridge_reconnect(self._handle)

    def stop(self):
        self._library.flint_bridge_stop(self._handle)

    def close(self):
        if self._handle:
            self._library.flint_bridge_destroy(self._handle)
            self._handle = None
