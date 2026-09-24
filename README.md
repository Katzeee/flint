# flint

flint connects running applications to a shared code-execution service, with a desktop window, system tray, and CLI in one executable. It belongs to the same stone-themed product line as lode.

For a source checkout, see [Development environment and first build](docs/development.md). For implementation and tests, see the [development guide](docs/contributing.md).

## Run flint

On Windows, install WebView2 and the Microsoft Visual C++ x64 runtime. Put flint.exe on PATH or invoke its full path.

Opening flint without arguments starts or reuses the backend and opens its window. Closing the window hides it while the backend and tray keep running.

```text
flint
flint status --json
flint restart --json
flint stop --json
```

Commands that need the backend start it automatically. Multiple terminals share that backend. Stop and restart refuse while execution responses are pending and leave host applications running. Submitted code is not automatically replayed after a communication failure.

Use `flint --help` or `flint <command> --help` for options. Endpoint and timeout options follow the command. A remote backend must already be running; automatic startup is local.

Workflow records and logs are stored in flint's local application-data directory. FLINT_STATE_DIR selects another directory; use the same value for commands controlling the same backend. Completed records survive a backend restart.

## Connect an application

Load the appropriate Bridge in the application you want to control. Follow the [Python Bridge connection guide](bridges/python/README.md) for Maya, 3ds Max, Blender, and Python, including Blender's installable Add-on, or the [.NET Bridge connection guide](bridges/dotnet/README.md) for Unity.

```text
flint hosts --json
flint instances --json
```

hosts discovers running applications; instances lists registered connections. Discovery alone does not establish a connection. Use a connected instance's ID when submitting code.

## Execute and inspect

List connected instances, create a workflow, and use the returned identifiers to submit code:

```text
flint workflow --name "My task" --json
flint exec --instance-id <instance-id> --workflow-id <workflow-id> --file <script-file> --json
flint execution --workflow-id <workflow-id> --execution-id 0001 --view full --json
```

Submit code understood by the selected instance. exec accepts exactly one of --code, --file, or --stdin. Reuse a workflow ID to group related executions. The execution command returns recorded stdout, stderr, errors, and status; --view full also includes source code.

Long executions return status running after five seconds and continue after the CLI exits. Query execution again to see incremental output and the final result. Exit code 0 includes accepted running work; it does not by itself mean host code has finished. A timeout or lost connection does not establish that host code has stopped.
