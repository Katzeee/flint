"""Maya plug-in entry point for Flint Bridge."""
import os


def initializePlugin(plugin):
    from flint_bridge import connect

    port = int(os.environ.get("FLINT_MAYA_REGISTRY_PORT", "6321"))
    connect(host="maya", name="Maya", port=port)


def uninitializePlugin(plugin):
    from flint_bridge import disconnect

    if not disconnect():
        raise RuntimeError("Flint Bridge is still executing host code")
