# 3ds Max Bridge

Export `flint-max.zip` with `flint bridge export max`. Extract `Flint.bundle` into an ApplicationPlugins search directory, such as `%APPDATA%/Autodesk/ApplicationPlugins`. With **Load Startup Scripts** enabled in 3ds Max's MAXScript preferences, the bundle's post-startup script connects to Bridge port `6321` when 3ds Max starts. Use **Flint > Flint Bridge** inside 3ds Max to edit the connection, inspect its status, or reconnect. Clicking **Apply** saves the settings in `FlintBridge.ini` under 3ds Max's user data directory.

Alternatively, export the Python bundle with `flint bridge export python`, start the backend with `flint start`, and run this in 3ds Max's Python execution environment on the application's main thread. Replace the ZIP path with the exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.max import manager

bridge = manager.connect(name="My Max")
```

Code submitted through flint executes on the application's UI thread.
