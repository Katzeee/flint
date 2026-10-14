# Maya Bridge

Export `flint-maya.zip` with `flint bridge export maya` and extract it into a directory on Maya's module search path. For example, place `flint.mod` and the adjacent `flint` directory under `Documents/maya/modules`. In Maya's Plug-in Manager, load `flint_plugin.py` and enable Auto load to connect on future launches. Use **Flint > Connection Settings** for the shared [connection settings](../../README.md#connection-settings). Unloading the plug-in disconnects it.

Alternatively, export the Python bundle with `flint bridge export python` and run this in Maya's Python script editor. Replace the ZIP path with the exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.maya import manager

bridge = manager.connect(name="My Maya")
```

Keep Maya open and responsive while running Python code through Flint.
