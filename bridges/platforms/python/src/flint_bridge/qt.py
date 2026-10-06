"""Resolve the Qt binding shared by a host's execution and UI components."""
from importlib import import_module
import sys
from types import SimpleNamespace


def resolve_qt(fallback=None):
    """Reuse a running binding; only a host may select an unloaded fallback."""
    for binding in ("PySide2", "PySide6"):
        core = sys.modules.get(binding + ".QtCore")
        widgets = sys.modules.get(binding + ".QtWidgets")
        if core is not None and widgets is not None and widgets.QApplication.instance() is not None:
            return SimpleNamespace(binding=binding, QtCore=core, QtWidgets=widgets)
    if fallback is not None:
        core = import_module(fallback + ".QtCore")
        widgets = import_module(fallback + ".QtWidgets")
        if widgets.QApplication.instance() is not None:
            return SimpleNamespace(binding=fallback, QtCore=core, QtWidgets=widgets)
    raise RuntimeError("Host has no running Qt application")
