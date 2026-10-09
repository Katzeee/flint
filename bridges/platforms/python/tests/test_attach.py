import json
import sys
import threading
from types import ModuleType, SimpleNamespace

import pytest

from flint_bridge import attach


@pytest.fixture
def host(monkeypatch):
    module = ModuleType("flint_test_host")
    monkeypatch.setitem(sys.modules, module.__name__, module)
    return module


def request(tmp_path):
    return json.dumps(
        dict(
            module="flint_test_host", address="127.0.0.1", port=6321, name="场景", error_path=str(tmp_path / "42.error")
        )
    )


def test_attach_runs_the_host_manager_on_a_daemon_thread(host, tmp_path):
    calls = []
    host.manager = SimpleNamespace(attach=lambda **arguments: calls.append((arguments, threading.current_thread())))
    thread = attach.start(request(tmp_path))
    thread.join(3)
    assert thread.daemon and not thread.is_alive()
    assert calls == [(dict(address="127.0.0.1", port=6321, name="场景"), thread)]
    assert not (tmp_path / "42.error").exists()


def test_attach_reports_why_the_manager_could_not_attach(host, tmp_path):
    def refuse(**_):
        raise RuntimeError("attach failed 场景")

    host.manager = SimpleNamespace(attach=refuse)
    attach.start(request(tmp_path)).join(3)
    assert (tmp_path / "42.error").read_text(encoding="utf-8") == "attach failed 场景"
