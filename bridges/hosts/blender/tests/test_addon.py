import importlib.util
from pathlib import Path
import sys
from types import ModuleType, SimpleNamespace


def load_addon(monkeypatch, connect):
    addon_path = Path(__file__).resolve().parents[1] / "addon/__init__.py"
    spec = importlib.util.spec_from_file_location(
        "flint_blender", addon_path, submodule_search_locations=[str(addon_path.parent)])
    addon = importlib.util.module_from_spec(spec)
    events = []
    bpy = ModuleType("bpy")
    props = ModuleType("bpy.props")
    for name in ("BoolProperty", "IntProperty", "StringProperty", "PointerProperty"):
        setattr(props, name, lambda **kwargs: None)
    bpy.props = props
    bpy.types = SimpleNamespace(AddonPreferences=object, Operator=object,
                                PropertyGroup=object, Panel=object,
                                WindowManager=type("WindowManager", (), {}))
    bpy.utils = SimpleNamespace(register_class=lambda klass: None,
                                unregister_class=lambda klass: None)
    preferences = SimpleNamespace(
        address="127.0.0.1", port=6321, instance_name="Blender", enabled=True)
    draft = SimpleNamespace(
        address="127.0.0.1", port=6321, instance_name="Blender", enabled=True)
    bpy.context = SimpleNamespace(
        window_manager=SimpleNamespace(flint_bridge_draft=draft, windows=[]),
        preferences=SimpleNamespace(addons={
            "flint_blender": SimpleNamespace(preferences=preferences)}))
    timers = SimpleNamespace(register=lambda *args, **kwargs: None,
                             is_registered=lambda *args: True,
                             unregister=lambda *args: None)
    bpy.app = SimpleNamespace(timers=timers)
    manager = SimpleNamespace(
        connect=lambda **kwargs: events.append(("connect", kwargs)) or connect(),
        disconnect=lambda: events.append(("disconnect",)) or True,
        current=lambda: None,
        configure=lambda *args, **kwargs: events.append(("configure", args, kwargs)),
    )
    monkeypatch.setitem(sys.modules, "bpy", bpy)
    monkeypatch.setitem(sys.modules, "bpy.props", props)
    monkeypatch.setitem(sys.modules, "flint_blender", addon)
    platform = ModuleType("flint_blender.flint_bridge")
    platform.__path__ = []
    platform.BridgeCreationError = BridgeCreationError
    monkeypatch.setitem(sys.modules, "flint_blender.flint_bridge", platform)
    monkeypatch.setitem(sys.modules, "flint_blender.flint_bridge.blender", SimpleNamespace(manager=manager))
    spec.loader.exec_module(addon)
    return addon, bpy, events, preferences, draft


class BridgeCreationError(RuntimeError):
    pass


def test_addon_enable_connects_and_disable_disconnects(monkeypatch):
    addon, bpy, events, preferences, draft = load_addon(monkeypatch, lambda: None)
    addon.register()
    draft.port = 6330
    assert addon.FLINT_OT_apply_settings().execute(bpy.context) == {"FINISHED"}
    assert preferences.port == 6330
    addon.unregister()

    assert events == [
        ("connect", {"address": "127.0.0.1",
                     "port": 6321, "name": "Blender", "enabled": True}),
        ("configure", (), {"address": "127.0.0.1", "port": 6330,
                                    "name": "Blender", "enabled": True}),
        ("disconnect",),
    ]


def test_a_bridge_that_cannot_start_leaves_the_addon_usable(monkeypatch, capsys):
    def occupied():
        raise BridgeCreationError("Another Bridge already owns this process")

    addon, bpy, events, preferences, draft = load_addon(monkeypatch, occupied)
    addon.register()
    assert "Another Bridge already owns this process" in capsys.readouterr().out
    assert addon.FLINT_OT_apply_settings().execute(bpy.context) == {"FINISHED"}
    assert [event[0] for event in events] == ["connect", "configure"]
