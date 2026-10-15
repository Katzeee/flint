# flint

flint connects running applications to a shared code-execution service, with a desktop window, system tray, and CLI in one executable.

To build Flint from source, follow [Development environment and first build](docs/development.md). To change its implementation or tests, follow the [contribution guide](docs/contributing.md).

## Run flint

On Windows, install WebView2 and the Microsoft Visual C++ x64 runtime. Put flint.exe on PATH or invoke its full path.

Opening flint without arguments starts or reuses the backend and opens the desktop window. The desktop and backend run in separate processes. Closing the window hides it in the tray; stopping the backend leaves the desktop open. Settings provides backend startup and shutdown. Quitting the desktop leaves the backend running.

```text
flint
flint status --json
flint stop --json
```

All terminals and agent sessions for the same user share one local backend. Commands that need the backend start it automatically, and so does opening the desktop, including launching flint while the desktop already runs. `flint status` and the desktop's status display only observe it, so a stopped backend stays stopped until the next such command or launch. Stop refuses while executions are active and leaves host applications running. Submitted code is not automatically replayed after a communication failure.

Use `flint --help` or `flint <command> --help` for options. Commands find the local backend automatically; no endpoint selection is required.

Workflow records, runtime locks, and logs live in flint's local application-data directory. Completed records survive a backend restart.

## Connect an application

Load the appropriate Bridge in the application you want to control. Follow the [Bridge guides](bridges/README.md) for host installation and connection instructions.

```text
flint apps --json
```

apps lists host applications in one place: instances connected to the backend and supported processes discovered on this machine. Each entry's `connection` is `ready` when it can run code, `connecting` while its execution channel is not yet established, or `not_connected` for a discovered process without a Bridge. Discovery alone does not establish a connection. Process operations such as attach take an entry's `pid`; submitting code takes its `instance_id`.

Alternatively, on Windows, attach injects the Bridge into a running host so it connects without loading any flint code yourself:

```text
flint attach --pid <pid>
```

attach starts a Bridge inside the process and prints its apps entry once it connects. A host process runs at most one Bridge. Attaching to an existing Bridge applies the requested settings. Use the ID returned by attach for subsequent requests; it remains stable for the host process's lifetime. Pass `--host-kind` for a process discovery does not classify (such as a plain Python interpreter), and `--name` to label the instance. For supported hosts, in-host loading, and host-specific behavior, follow the Bridge guides above.

The desktop's application discovery, inspection, and window switching use the same local services as the CLI. Use `flint apps info --help` and `flint apps focus --help` to inspect or focus a local application by PID. Information queries return process identity and window metadata without capturing an image. Request a preview explicitly when visual context is needed; image capture failure leaves the process information available and includes the reason. Minimized and headless processes remain discoverable. Inspection does not restore windows; window switching is an explicit action. Unity asset import workers are internal editor processes and are excluded from discovery.

## Execute and inspect

Find a ready application with `flint apps`, create a workflow, and use the returned identifiers to submit code:

```text
flint workflow --name "My task" --json
flint exec --instance-id <instance-id> --workflow-id <workflow-id> --code "print(1 + 1)" --json
flint execution --workflow-id <workflow-id> --execution-id 0001 --view full --json
```

Submit code understood by the selected instance; the example above uses Python. For input selection and PowerShell multiline examples, see `flint exec --help`.

Reuse a workflow ID to group related executions. Query the execution to read recorded stdout, stderr, errors, and status; `--view full` also includes source code.

`exec` waits for completion by default. With `--detach`, it returns after submission; exit code 0 means the backend accepted the task.
