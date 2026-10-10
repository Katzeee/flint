# Blender Bridge

Export an installable Blender Add-on ZIP with `flint bridge export blender`. In Blender 5.2, open **Edit > Preferences > Add-ons**, choose **Install from Disk**, select `flint-blender.zip`, and enable **Flint Bridge**. Open its preferences or the **Flint** tab in the 3D View sidebar to inspect the connection, edit its address, port, and instance name, connect, or disconnect. The fields are drafts; **Apply** saves settings for the next connection without changing the current one. **Connect** uses the saved settings; **Disconnect** stops the connection and automatic retries. When saved and active settings differ, the panel notes that the new settings take effect on the next connection. Disabling the Add-on disconnects it. `flint instances --json` shows the registered instance.

Alternatively, export the Python bundle with `flint bridge export python`, start the backend with `flint start`, and run this in Blender's Python Console on the application's main thread. Replace the ZIP path with the exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.blender import manager

bridge = manager.connect(name="My Blender")
```

Both connection methods register a Blender timer on the main thread and execute submitted code there. Keep Blender's event loop running while the Bridge is connected. Code may use `bpy` to work with the open scene.
