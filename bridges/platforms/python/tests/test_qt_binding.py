import sys
from types import SimpleNamespace

import pytest

from flint_bridge import qt


@pytest.mark.parametrize("active, fallback", [("PySide2", "PySide6"), ("PySide6", "PySide2")])
def test_running_binding_wins_over_another_available_binding(monkeypatch, active, fallback):
    core = SimpleNamespace()
    widgets = SimpleNamespace(QApplication=SimpleNamespace(instance=lambda: object()))
    monkeypatch.setitem(sys.modules, fallback + ".QtCore", SimpleNamespace())
    monkeypatch.setitem(
        sys.modules, fallback + ".QtWidgets", SimpleNamespace(QApplication=SimpleNamespace(instance=lambda: None))
    )
    monkeypatch.setitem(sys.modules, active + ".QtCore", core)
    monkeypatch.setitem(sys.modules, active + ".QtWidgets", widgets)
    imported = []
    monkeypatch.setattr(qt, "import_module", lambda name: imported.append(name))
    selected = qt.resolve_qt(fallback=fallback)
    assert selected.QtCore is core
    assert selected.QtWidgets is widgets
    assert imported == []
