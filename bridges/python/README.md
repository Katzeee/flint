# Python Bridge

The Python Bridge connects an application's Python runtime to flint. It supports Maya, 3ds Max, Blender, and plain Python processes through active connections initiated inside the host. flint itself does not require a Python installation; the Bridge uses the host's interpreter.

## Prepare the Bridge

Export the complete Bridge ZIP and start the backend:

```text
flint bridge export python
flint start
```

The ZIP includes the Python adapter and the platform's native Bridge core in the same `flint_bridge` package. It needs no third-party Python packages and can be loaded without a development checkout or pip installation; on first connection it extracts the DLL to a versioned temporary directory. Make the ZIP accessible to the host, then run this shared setup inside that host's Python environment. Replace the path with the absolute path to your exported ZIP:

```python
import sys
sys.path.insert(0, "C:/tools/flint-python.zip")

import flint_bridge
```

## Maya

Run the shared setup and this connection call from Maya's Python script editor on the application's main thread:

```python
bridge = flint_bridge.connect(host="maya", name="My Maya")
```

This integration is validated with Maya 2024. Code submitted through flint executes on Maya's UI thread.

## 3ds Max

Run the shared setup and this connection call in 3ds Max's Python execution environment on the application's main thread:

```python
bridge = flint_bridge.connect(host="max", name="My Max")
```

This integration is validated with 3ds Max 2024.2.13. Code submitted through flint executes on the application's UI thread.

## Blender

Export an installable Blender Add-on ZIP with `flint bridge export blender`. In Blender 5.2, open **Edit > Preferences > Add-ons**, choose **Install from Disk**, select `flint-blender.zip`, and enable **Flint Bridge**. The Add-on connects to the local backend on registry port `6321` when enabled; `flint instances --json` shows the registered instance. Set `FLINT_BLENDER_REGISTRY_PORT` in Blender's environment before launch if the backend uses another registry port. Disabling the Add-on disconnects it.

Alternatively, run the shared setup and this connection call in Blender's Python Console on the application's main thread:

```python
bridge = flint_bridge.connect(host="blender", name="My Blender")
```

This integration is validated with Blender 5.2. Both connection methods register a Blender timer on the main thread and execute submitted code there. Keep Blender's event loop running while the Bridge is connected. Code may use `bpy` to work with the open scene.

## Plain Python

After the shared setup, connect a script or interactive Python process with:

```python
bridge = flint_bridge.connect(host="python", name="My Python process")
```

Keep the process running while using the connection. The Bridge runs in background threads and does not keep an otherwise finished script alive. Submitted code runs on a worker thread.

## Connection and execution

connect uses the local backend by default and accepts address and port for another endpoint. Repeated calls reuse the matching Bridge. bridge.connected reports readiness; bridge.wait_until_connected(timeout=10) waits for the connection when needed. Use flint instances --json to retrieve the assigned instance ID.

Each Bridge executes one request at a time and retains its Python execution namespace across requests. Writes to `sys.stdout` and `sys.stderr` on that execution thread stream into the request's output; writes from other host threads continue to their original streams. Threads started by submitted code are not attributed to the execution automatically. Follow the [flint execution guide](../../README.md#execute-and-inspect) to submit code and retrieve results.

Call flint_bridge.disconnect() before changing the endpoint. A false return indicates that shutdown has not completed. Disconnecting stops transport and cancels queued work, but does not forcibly interrupt already running host code.
