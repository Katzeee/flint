"""The injected bridge must use the Qt binding already running in its host."""

import sys
from types import SimpleNamespace

import pytest

from flint_bridge.hosts import qt


@pytest.fixture
def pyside2(monkeypatch):
    ui_thread = object()
    core = SimpleNamespace(QThread=SimpleNamespace(currentThread=lambda: ui_thread))
    widgets = SimpleNamespace(QApplication=SimpleNamespace(
        instance=lambda: SimpleNamespace(thread=lambda: ui_thread)))
    monkeypatch.setattr(qt, "QtMainThreadExecutionStrategy", lambda qt: qt)
    return core, widgets


def test_prefers_the_hosts_running_pyside2_even_when_pyside6_is_present(monkeypatch, pyside2):
    core, widgets = pyside2
    monkeypatch.setitem(sys.modules, "PySide2.QtCore", core)
    monkeypatch.setitem(sys.modules, "PySide2.QtWidgets", widgets)
    selected = qt.create_strategy()
    assert selected.QtCore is core
    assert selected.QtWidgets is widgets


def test_initializes_pyside2_if_host_has_not_imported_its_binding(monkeypatch, pyside2):
    core, widgets = pyside2
    monkeypatch.delitem(sys.modules, "PySide2.QtCore", raising=False)
    monkeypatch.delitem(sys.modules, "PySide2.QtWidgets", raising=False)
    monkeypatch.setitem(sys.modules, "PySide2", SimpleNamespace(QtCore=core, QtWidgets=widgets))
    selected = qt.create_strategy()
    assert selected.QtCore is core
    assert selected.QtWidgets is widgets
