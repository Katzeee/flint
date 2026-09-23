"""The injected bridge must use the Qt binding already running in its host."""

import sys
from types import SimpleNamespace

from flint_bridge.hosts import qt


def test_prefers_the_hosts_running_pyside2_even_when_pyside6_is_present(monkeypatch):
    ui_thread = object()
    core = SimpleNamespace(QThread=SimpleNamespace(currentThread=lambda: ui_thread))
    widgets = SimpleNamespace(QApplication=SimpleNamespace(
        instance=lambda: SimpleNamespace(thread=lambda: ui_thread)))
    monkeypatch.setitem(sys.modules, "PySide2.QtCore", core)
    monkeypatch.setitem(sys.modules, "PySide2.QtWidgets", widgets)
    monkeypatch.setattr(qt, "QtMainThreadExecutionStrategy", lambda qt: qt)
    selected = qt.create_strategy()
    assert selected.QtCore is core
    assert selected.QtWidgets is widgets


def test_initializes_pyside2_if_host_has_not_imported_its_binding(monkeypatch):
    ui_thread = object()
    core = SimpleNamespace(QThread=SimpleNamespace(currentThread=lambda: ui_thread))
    widgets = SimpleNamespace(QApplication=SimpleNamespace(
        instance=lambda: SimpleNamespace(thread=lambda: ui_thread)))
    monkeypatch.delitem(sys.modules, "PySide2.QtCore", raising=False)
    monkeypatch.delitem(sys.modules, "PySide2.QtWidgets", raising=False)
    monkeypatch.setitem(sys.modules, "PySide2", SimpleNamespace(QtCore=core, QtWidgets=widgets))
    monkeypatch.setattr(qt, "QtMainThreadExecutionStrategy", lambda qt: qt)
    selected = qt.create_strategy()
    assert selected.QtCore is core
    assert selected.QtWidgets is widgets
