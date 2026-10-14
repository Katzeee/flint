# 3ds Max Bridge

Export `flint-max.zip` with `flint bridge export max`. Extract `Flint.bundle` into an ApplicationPlugins search directory, such as `%APPDATA%/Autodesk/ApplicationPlugins`. With **Load Startup Scripts** enabled in 3ds Max's MAXScript preferences, the bundle connects to the local backend when 3ds Max starts. Use **Flint > Flint Bridge** for the shared [connection settings](../../README.md#connection-settings).

Alternatively, export the Python bundle with `flint bridge export python` and run this in 3ds Max's Python execution environment. Replace the ZIP path with the exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.max import manager

bridge = manager.connect(name="My Max")
```

Keep 3ds Max open and responsive while running Python code through Flint.
