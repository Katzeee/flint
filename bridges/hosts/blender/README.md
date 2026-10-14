# Blender Bridge

Export an installable Blender Add-on ZIP with `flint bridge export blender`. In Blender 5.2, open **Edit > Preferences > Add-ons**, choose **Install from Disk**, select `flint-blender.zip`, and enable **Flint Bridge**. Open its preferences or the **Flint** tab in the 3D View sidebar for the shared [connection settings](../../README.md#connection-settings). Disabling the Add-on disconnects it.

Alternatively, export the Python bundle with `flint bridge export python` and run this in Blender's Python Console. Replace the ZIP path with the exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.blender import manager

bridge = manager.connect(name="My Blender")
```

Keep Blender open and responsive while running code through Flint. Submit Python code; use `bpy` to work with the open scene.
