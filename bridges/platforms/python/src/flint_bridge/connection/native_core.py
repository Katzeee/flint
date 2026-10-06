"""C ABI access to the shared host-side connection core."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import pkgutil
import sys
import tempfile

from .errors import BridgeBusyError, BridgeCreationError, BridgeStoppedError

# Creation error codes from the native core.
_CREATION_ERRORS = {1: "invalid_configuration", 2: "claimed", 3: "system"}


def _library_name():
    if sys.platform == "win32":
        return "flint_bridge_core.dll"
    if sys.platform == "darwin":
        return "libflint_bridge_core.dylib"
    return "libflint_bridge_core.so"


def _library_path():
    data = pkgutil.get_data(__package__.rsplit(".", 1)[0], _library_name())
    if data is None:
        raise BridgeCreationError("library_unavailable", "Bridge package has no native connection core")
    digest = hashlib.sha256(data).hexdigest()[:20]
    folder = Path(tempfile.gettempdir()) / "flint-bridge" / digest
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / _library_name()
    if target.exists():
        if target.read_bytes() != data:
            raise BridgeCreationError(
                "library_unavailable", "Extracted Bridge core does not match the package")
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
    try:
        library = ctypes.CDLL(str(_library_path()))
    except OSError as error:
        raise BridgeCreationError("library_unavailable", "Cannot load Bridge core: {}".format(error))
    try:
        _bind(library)
    except AttributeError as error:
        raise BridgeCreationError("abi_mismatch", "Bridge core is missing {}".format(error))
    if library.flint_bridge_abi_version() != 4:
        raise BridgeCreationError("abi_mismatch", "Unsupported native Bridge ABI")
    return library


def _bind(library):
    library.flint_bridge_abi_version.restype = ctypes.c_uint32
    library.flint_bridge_create.argtypes = [
        ctypes.c_char_p, ctypes.POINTER(ctypes.c_uint32), ctypes.POINTER(ctypes.c_void_p)]
    library.flint_bridge_create.restype = ctypes.c_void_p
    library.flint_bridge_poll.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    library.flint_bridge_poll.restype = ctypes.c_void_p
    library.flint_bridge_report_execution.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
    library.flint_bridge_report_execution.restype = ctypes.c_bool
    library.flint_bridge_connected.argtypes = [ctypes.c_void_p]
    library.flint_bridge_connected.restype = ctypes.c_bool
    library.flint_bridge_busy.argtypes = [ctypes.c_void_p]
    library.flint_bridge_busy.restype = ctypes.c_bool
    library.flint_bridge_stopped.argtypes = [ctypes.c_void_p]
    library.flint_bridge_stopped.restype = ctypes.c_bool
    library.flint_bridge_instance_id.argtypes = [ctypes.c_void_p]
    library.flint_bridge_instance_id.restype = ctypes.c_void_p
    library.flint_bridge_status_json.argtypes = [ctypes.c_void_p]
    library.flint_bridge_status_json.restype = ctypes.c_void_p
    library.flint_bridge_reconnect.argtypes = [ctypes.c_void_p]
    library.flint_bridge_reconnect.restype = ctypes.c_bool
    library.flint_bridge_apply_settings.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
    library.flint_bridge_apply_settings.restype = ctypes.c_uint32
    library.flint_bridge_stop.argtypes = [ctypes.c_void_p]
    library.flint_bridge_destroy.argtypes = [ctypes.c_void_p]
    library.flint_bridge_string_free.argtypes = [ctypes.c_void_p]


class NativeCore:
    def __init__(self, config):
        self._library = _load()
        encoded = json.dumps(config, ensure_ascii=False).encode("utf-8")
        kind = ctypes.c_uint32()
        error = ctypes.c_void_p()
        self._handle = self._library.flint_bridge_create(
            encoded, ctypes.byref(kind), ctypes.byref(error))
        message = self._string(error.value)
        if not self._handle:
            raise BridgeCreationError(_CREATION_ERRORS.get(kind.value, "system"),
                                      message or "Cannot start native Bridge core")

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

    def report_execution(self, report):
        encoded = json.dumps(report, ensure_ascii=False).encode("utf-8")
        return self._library.flint_bridge_report_execution(self._handle, encoded)

    @property
    def connected(self):
        return bool(self._library.flint_bridge_connected(self._handle))

    @property
    def busy(self):
        return bool(self._library.flint_bridge_busy(self._handle))

    @property
    def instance_id(self):
        return self._string(self._library.flint_bridge_instance_id(self._handle)) or None

    @property
    def status(self):
        return json.loads(self._string(self._library.flint_bridge_status_json(self._handle)))

    def reconnect(self):
        if not self._library.flint_bridge_reconnect(self._handle):
            raise BridgeStoppedError()

    def check_running(self):
        if self._library.flint_bridge_stopped(self._handle):
            raise BridgeStoppedError()

    def apply_settings(self, settings):
        encoded = json.dumps(settings, ensure_ascii=False).encode("utf-8")
        result = self._library.flint_bridge_apply_settings(self._handle, encoded)
        if result == 1:
            raise BridgeBusyError("Bridge is executing host code")
        if result == 2:
            raise ValueError("Invalid Bridge connection settings")
        if result == 3:
            raise BridgeStoppedError()
        if result != 0:
            raise RuntimeError("Unknown Bridge settings result")

    def stop(self):
        self._library.flint_bridge_stop(self._handle)

    def close(self):
        if self._handle:
            self._library.flint_bridge_destroy(self._handle)
            self._handle = None
