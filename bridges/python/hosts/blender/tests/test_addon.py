import importlib.util
from pathlib import Path
import sys
from types import SimpleNamespace


def test_addon_enable_connects_and_disable_disconnects(monkeypatch):
    addon_path = Path(__file__).resolve().parents[1] / "addon/__init__.py"
    spec = importlib.util.spec_from_file_location(
        "flint_blender", addon_path, submodule_search_locations=[str(addon_path.parent)])
    addon = importlib.util.module_from_spec(spec)
    events = []
    bridge = SimpleNamespace(
        connect=lambda **kwargs: events.append(("connect", kwargs)),
        disconnect=lambda: events.append(("disconnect",)) or True,
    )
    monkeypatch.setitem(sys.modules, "flint_blender", addon)
    monkeypatch.setitem(sys.modules, "flint_blender.flint_bridge", bridge)
    monkeypatch.delenv("FLINT_BLENDER_REGISTRY_PORT", raising=False)
    spec.loader.exec_module(addon)

    addon.register()
    addon.unregister()

    assert events == [
        ("connect", {"host": "blender", "name": "Blender", "port": 6321}),
        ("disconnect",),
    ]
