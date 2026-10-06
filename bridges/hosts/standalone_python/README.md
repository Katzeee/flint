# Standalone Python Bridge

Export the bundle with `flint bridge export python` and start the backend with `flint start`. The bundle contains the Python integration and native Bridge core and needs no pip installation. Load it inside the Python process you want to control, replacing the ZIP path with your exported file:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")
from flint_bridge.standalone_python import manager

bridge = manager.connect(name="My Python process")
```

Keep the process running while using the Bridge. The Bridge uses background threads and does not keep an otherwise finished script alive. Submitted code runs on a worker thread and retains its Python namespace between requests. Output from that execution thread is returned to Flint; output from unrelated host threads remains with the host. Follow the [execution guide](../../../README.md#execute-and-inspect) to submit code and inspect results.

Repeated connection calls reuse the matching Bridge. Use the Bridge's status to inspect readiness and the reason for a retry. The host integration's configure and reconnect methods update settings or retry the connection without replacing the execution context. When finishing, call `manager.disconnect()`; a false return means shutdown is still waiting for running code and resources cannot yet be released.
