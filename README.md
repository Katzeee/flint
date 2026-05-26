# python-bridge-mcp

A local MCP + Python execution bridge.

It has three parts:

- `python_bridge_mcp.server.shim`: The stdio MCP entry point started by the MCP client (thin shim). On startup it ensures the backend is available and proxies MCP tool requests.
- `python_bridge_mcp.server.backend`: A shared background service that holds the instance registry and control API. Multiple MCP client instances share one backend process.
- `python_bridge_mcp.client`: Runs inside any Python process (e.g. a DCC application or a standalone script). Registers the process with the backend and executes code on request.

Current features:

- Python instances actively register themselves with the backend
- Multiple Python instances can be online simultaneously
- Multiple MCP client instances share a single backend process (shared instance registry)
- The shim auto-starts the backend on startup; if the backend crashes, it is relaunched on the next shim start
- Per-instance serial execution
- `exec_python` returns `stdout`, `stderr`, and `traceback`
- Execution runs in an isolated namespace
- Structured workflow recording: multiple `exec_python` calls are grouped into a single workflow JSON file
- 5-second early return: long-running executions do not block the MCP caller; they return `status: "running"` immediately and can be polled via lookup tools
- 500 ms periodic output flush from the client side; in-flight output is visible in the workflow file incrementally
- Cross-instance workflows: a single workflow can include executions on different Python instances
- Workflow file persistence: after a backend restart, the lookup tools recover workflow state from disk

## Quick Start

For normal use, the recommended approach is to build the packaged executables first and then point your MCP client directly at `python-bridge-mcp-shim.exe`.

### 1. Set up the environment

```powershell
tools\setup_env.bat
```

This script will:

- Check that `python` is available on `PATH`
- Create a `.venv` in the repository root if one does not exist
- Install development and build dependencies: `.[dev,build]`

### 2. Build the packaged executables

```powershell
tools\bundle\build.bat
```

After the build, the outputs are at:

- `dist/python-bridge-mcp/python-bridge-mcp-shim.exe`
- `dist/python-bridge-mcp/python-bridge-mcp-backend.exe`

`tools\bundle\build.bat` only handles packaging; it always uses the `.venv` in the repository root.

If the build fails with an error about not being able to clean the output directory, the old `python-bridge-mcp-shim.exe` or `python-bridge-mcp-backend.exe` is still running — stop it and retry.

### 3. Add the output directory to `PATH`

Add the following directory to your user or system `PATH`:

- `dist\python-bridge-mcp`

Notes:

- `python-bridge-mcp-shim.exe` and `python-bridge-mcp-backend.exe` must live in the same directory
- The MCP client only needs to launch `python-bridge-mcp-shim.exe`
- `python-bridge-mcp-shim.exe` looks for `python-bridge-mcp-backend.exe` next to itself and starts it automatically

### 4. Configure your MCP client

For Cursor (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "python-bridge-mcp": {
      "type": "stdio",
      "command": "python-bridge-mcp-shim.exe",
      "args": []
    }
  }
}
```

After saving the file:

1. Reload MCP in your client
2. Confirm that `list_instances`, `exec_python`, and the other tools are visible in the agent's tool list

If the MCP loads correctly but `list_instances` returns an empty list, that means no Python instance has registered yet — not a configuration problem.

### 5. Start the client inside a Python process

The minimal integration is to call `start_control_client_service` once after your process starts:

```python
import threading
from python_bridge_mcp.client.bootstrap import start_control_client_service

start_control_client_service(
    instance_id="my-script-001",   # unique ID for this process
    instance_name="My Script",     # human-readable label
    instance_type="python",        # arbitrary type tag, e.g. "maya", "max", "blender"
)
```

Notes:

- `instance_id` must be unique across all running instances
- `instance_type` is used for filtering with `list_instances(instance_type=...)`
- The default registry endpoint is `127.0.0.1:6321`
- `start_control_client_service` is reload-safe: calling it again replaces the existing service

To stop the service:

```python
from python_bridge_mcp.client.bootstrap import stop_control_client_service

stop_control_client_service()
```

For Qt applications where code must run on the main thread, import `QtMainThreadRunner` from `python_bridge_mcp.client.code_runner` and pass it as the `runner` argument. The default runner auto-detects Qt; if no Qt is found it falls back to `DirectRunner`.

### 6. Available tools and typical workflow

Tools available to the agent:

- `list_instances(instance_type=None)` — list registered Python instances
- `start_workflow(name, description?)` — create a workflow, returns `workflow_id`
- `exec_python(instance_id, code, workflow_id, name?)` — execute code on an instance
- `get_workflow_execution(workflow_id, execution_id, view?)` — show details of a single execution

A typical session looks like:

1. Call `list_instances` to see which Python processes are online
2. Call `start_workflow` to create a workflow for the current task
3. Call `exec_python` to send code to the chosen instance
4. If execution takes a while, retrieve the `execution_id` from the result and poll with `get_workflow_execution`

Practical tips:

- `instance_id` comes from the `instance_id` field in `list_instances` results
- Pass a complete Python source string to `code`, not a file path
- Reuse the same `workflow_id` across all steps of one task
- `name` in `exec_python` helps identify each step in a multi-step workflow

## Development

### Environment setup

Python 3.10 is recommended for the server. Shared and client modules maintain Python 3.7 compatibility.

```powershell
tools\setup_env.bat
```

Or manually:

```powershell
python -m venv .venv
.venv\Scripts\activate
pip install -e .[dev,build]
```

### Development-mode MCP client config

For Cursor (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "python-bridge-mcp": {
      "type": "stdio",
      "command": "${workspaceFolder}/.venv/Scripts/python.exe",
      "args": ["-X", "utf8", "-m", "python_bridge_mcp.server.shim"]
    }
  }
}
```

`-X utf8` prevents GBK encoding issues on Chinese-locale Windows systems where Python defaults stderr to GBK while the MCP client reads UTF-8.

### Manual startup

In most cases you do not need to start the backend manually — the shim does it automatically.

To debug the backend directly:

```powershell
.venv\Scripts\activate
python -X utf8 -m python_bridge_mcp.server.backend
```

Or start only the shim (it will launch the backend for you):

```powershell
python -X utf8 -m python_bridge_mcp.server.shim
```

### Debug CLI

`client/cli.py` is the human-operable equivalent of the MCP shim. It connects to the running backend and lets you list instances and execute code or files without an AI agent.

It is designed as a standalone script with no package context required — only stdlib is needed, so it runs under any Python interpreter including `mayapy` or `3dsmaxpy`:

```powershell
# List online instances
python src\python_bridge_mcp\client\cli.py list

# Execute a code snippet
python src\python_bridge_mcp\client\cli.py exec --instance-id maya-1234 --code "print('hello')"

# Execute a file (original path is passed to compile() so debugpy breakpoints work)
python src\python_bridge_mcp\client\cli.py exec --instance-id maya-1234 --file path\to\script.py
```

`--host` and `--port` override the backend address (default `localhost:6322`).

## Architecture

```
MCP Client A ──stdio──> shim ──TCP:6322──> backend ──TCP:6321──> Python instance
MCP Client B ──stdio──> shim ──────────────┘
```

- **shim** (`python -m python_bridge_mcp.server.shim`): one per MCP client process. Speaks the MCP stdio protocol and ensures the backend is running before forwarding tool calls.
- **backend** (`python -m python_bridge_mcp.server.backend`): one shared instance. Binds `127.0.0.1:6322` (control API) and `127.0.0.1:6321` (instance registry).
- **client** (`python_bridge_mcp.client`): one per Python process. Connects to the registry, sends heartbeats, and executes incoming code requests.

## Project Structure

```text
python-bridge-mcp/
├─ src/python_bridge_mcp/
│  ├─ client/
│  │  ├─ bootstrap.py           # start/stop_control_client_service lifecycle helpers
│  │  ├─ cli.py                 # debug CLI — human-operable equivalent of the shim
│  │  ├─ code_executor.py       # low-level code execution with stdout/stderr capture
│  │  ├─ code_runner.py         # runner abstraction: DirectRunner and QtMainThreadRunner
│  │  ├─ discovery.py           # registry client: registration, heartbeat, exec dispatch
│  │  └─ __init__.py
│  ├─ server/
│  │  ├─ backend.py             # entry point — spawns Registry and ControlServer
│  │  ├─ backend_client.py      # async client for the control API (used by shim)
│  │  ├─ control_models.py      # control API wire models and error codes
│  │  ├─ control_server.py      # TCP API server: workflow and execution management
│  │  ├─ launcher.py            # backend auto-launch logic for the shim
│  │  ├─ registry.py            # instance discovery, heartbeat, and request forwarding
│  │  ├─ shim.py                # stdio MCP entry: ensures backend running, exposes tools
│  │  └─ __init__.py
│  ├─ shared/
│  │  ├─ backend_client.py      # minimal sync client for the control API (used by cli.py)
│  │  ├─ constants.py           # shared network constants (ports, default host)
│  │  ├─ file_writer.py         # thread-safe atomic file writes via filelock
│  │  ├─ instance_control_models.py  # wire protocol between client and registry
│  │  ├─ jsonline.py            # JSON-line socket codec (sync + async)
│  │  ├─ model_base.py          # base dataclass model with serialization
│  │  ├─ text_buffer.py         # thread-safe stdout/stderr accumulation buffer
│  │  ├─ workflow_models.py     # persistent workflow data structures
│  │  ├─ workflow_persistence.py # disk-based workflow storage (platformdirs)
│  │  └─ __init__.py
│  └─ __init__.py
├─ tests/
│  ├─ client/
│  ├─ server/
│  ├─ integration/
│  └─ conftest.py
├─ tools/
│  ├─ bundle/
│  │  ├─ build.bat              # Windows build wrapper
│  │  ├─ build.py               # PyInstaller orchestration script
│  │  ├─ windows_bundle.spec    # PyInstaller spec: python-bridge-mcp-shim.exe + python-bridge-mcp-backend.exe
│  │  └─ entrypoints/
│  │     ├─ backend_entry.py    # python-bridge-mcp-backend.exe entry point
│  │     └─ shim_entry.py       # python-bridge-mcp-shim.exe entry point
│  └─ setup_env.bat             # venv creation and dependency installation
├─ docs/
└─ pyproject.toml
```

## Testing

```powershell
.venv\Scripts\activate
pytest tests/ -v
```

Current test coverage:

- `client` unit tests (discovery, code runner, code executor, bootstrap)
- `server` unit tests (control models, control server, registry, shim, launcher)
- End-to-end registration, listing, and execution flows
- Backend restart with automatic client re-registration
- Multiple shim instances sharing one backend
