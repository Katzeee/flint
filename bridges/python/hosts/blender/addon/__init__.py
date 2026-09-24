"""Blender Add-on entry point for the bundled Flint Python Bridge."""
import os

bl_info = {
    "name": "Flint Bridge",
    "author": "Flint",
    "version": (0, 1, 0),
    "blender": (4, 2, 0),
    "location": "Preferences > Add-ons",
    "description": "Connect Blender to a running Flint backend",
    "category": "Development",
}


def register():
    from . import flint_bridge

    port = int(os.environ.get("FLINT_BLENDER_REGISTRY_PORT", "6321"))
    flint_bridge.connect(host="blender", name="Blender", port=port)


def unregister():
    from . import flint_bridge

    if not flint_bridge.disconnect():
        raise RuntimeError("Flint Bridge is still executing host code")
