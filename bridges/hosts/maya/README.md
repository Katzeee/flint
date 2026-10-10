# Maya Bridge

Export `flint-maya.zip` with `flint bridge export maya` and extract it into a directory on Maya's module search path. For example, place `flint.mod` and the adjacent `flint` directory under `Documents/maya/modules`. In Maya's Plug-in Manager, load `flint_plugin.py` and enable Auto load to connect on future launches. Use **Flint > Connection Settings** inside Maya to edit the address, port, and instance name, inspect the connection, connect, or disconnect. **Apply** saves settings for the next connection without changing the current one. **Connect** uses the saved settings; **Disconnect** stops the connection and automatic retries. When saved and active settings differ, the panel notes that the new settings take effect on the next connection. Unloading the plug-in disconnects it.

Alternatively, export the Python bundle with `flint bridge export python`, start the backend with `flint start`, and run this in Maya's Python script editor on the application's main thread. Replace the ZIP path with the exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.maya import manager

bridge = manager.connect(name="My Maya")
```

Code submitted through flint executes on Maya's UI thread.
