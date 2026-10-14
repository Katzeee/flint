# Standalone Python Bridge

Export the bundle with `flint bridge export python`; no pip installation is needed. Run this inside the Python process you want to control, replacing the ZIP path with your exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.standalone_python import manager

bridge = manager.connect(name="My Python process")
```

Keep the Python process running while using Flint; connecting does not keep a finished script alive. Variables defined by submitted code remain available to later submissions. Output from submitted code is returned to Flint; output from other threads is not captured.

Use `bridge.connected` to check the connection and `bridge.status` for connection details. Call `manager.reconnect()` to retry. When finishing, call `manager.disconnect()`; if it returns `False`, wait for running code to finish and call it again.
