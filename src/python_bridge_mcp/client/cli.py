"""Command-line interface for python-bridge-mcp.

Human-operable equivalent of the MCP shim: connects to the backend,
lists instances, and executes code or files on a specific instance.

Can be run as a standalone script (no package context required):
    python cli.py list
    python cli.py exec --instance-id ID (--code CODE | --file PATH)

Or via the installed entry point:
    python-bridge list
    python-bridge exec --instance-id ID ...
"""
import argparse
import os as _os
import sys
from pathlib import Path
from typing import Any, Dict, List, Optional

# Allow running as a standalone script without package context.
# Adds the dcc_listener root to sys.path so shared.* is importable directly.
if __package__ is None or __package__ == "":
    _ROOT = _os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))
    if _ROOT not in sys.path:
        sys.path.insert(0, _ROOT)
    from shared.backend_client import BackendClientError, BackendControlClient  # type: ignore
    from shared.constants import CONTROL_API_PORT, DEFAULT_HOST  # type: ignore
else:
    from ..shared.backend_client import BackendClientError, BackendControlClient
    from ..shared.constants import CONTROL_API_PORT, DEFAULT_HOST


def _make_client(args: argparse.Namespace) -> BackendControlClient:
    return BackendControlClient(host=args.host, port=args.port)


def cmd_list(args: argparse.Namespace) -> None:
    client = _make_client(args)
    try:
        instances = client.list_instances(getattr(args, "type", None))
    except OSError as exc:
        print(f"Cannot connect to backend at {args.host}:{args.port}: {exc}", file=sys.stderr)
        sys.exit(1)

    if not instances:
        print("No instances online.")
        return

    for inst in instances:
        itype = inst.get("instance_type") or ""
        itype_str = f"  [{itype}]" if itype else ""
        print(f"{inst['instance_id']}{itype_str}  {inst.get('instance_name', '')}")


def cmd_exec(args: argparse.Namespace) -> None:
    filename: Optional[str] = None
    if args.file:
        path = Path(args.file).resolve()
        if not path.exists():
            print(f"File not found: {args.file}", file=sys.stderr)
            sys.exit(1)
        code = path.read_text(encoding="utf-8")
        filename = str(path)
    else:
        code = args.code

    client = _make_client(args)
    try:
        wf_id = client.start_workflow("cli-exec")
        result = client.execute(
            instance_id=args.instance_id,
            code=code,
            workflow_id=wf_id,
            name=args.name or "",
            filename=filename,
        )
    except OSError as exc:
        print(f"Cannot connect to backend at {args.host}:{args.port}: {exc}", file=sys.stderr)
        sys.exit(1)
    except BackendClientError as exc:
        print(f"Backend error ({exc.error_code}): {exc}", file=sys.stderr)
        sys.exit(1)

    status = result.get("status")
    execution_id = result.get("execution_id")

    if status in ("pending", "running"):
        print(
            f"Execution running (workflow={wf_id}, execution={execution_id})",
            file=sys.stderr,
        )
        try:
            client.wait_for_completion(wf_id, execution_id)
        except TimeoutError as exc:
            print(str(exc), file=sys.stderr)
            sys.exit(1)
        except BackendClientError as exc:
            print(f"Backend error ({exc.error_code}): {exc}", file=sys.stderr)
            sys.exit(1)

    try:
        detail = client.get_execution(wf_id, execution_id, view="full")
    except BackendClientError as exc:
        print(f"Backend error ({exc.error_code}): {exc}", file=sys.stderr)
        sys.exit(1)

    _print_result(detail)


def _print_result(result: Dict[str, Any]) -> None:
    stdout = result.get("stdout") or ""
    stderr = result.get("stderr") or ""
    traceback = result.get("traceback")
    status = result.get("status")

    if stdout:
        print(stdout, end="")
    if stderr:
        print(stderr, end="", file=sys.stderr)
    if traceback:
        print(traceback, file=sys.stderr)
    if status == "failed":
        sys.exit(1)


def main(argv: Optional[List[str]] = None) -> None:
    parser = argparse.ArgumentParser(
        prog="python-bridge",
        description="python-bridge-mcp debug CLI",
    )
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=CONTROL_API_PORT)
    subparsers = parser.add_subparsers(dest="command")

    list_parser = subparsers.add_parser("list", help="List online instances")
    list_parser.add_argument("--type", dest="type", help="Filter by instance type")

    exec_parser = subparsers.add_parser("exec", help="Execute code on an instance")
    exec_parser.add_argument("--instance-id", required=True, dest="instance_id")
    exec_parser.add_argument("--name", help="Label for this execution in the workflow record")
    source = exec_parser.add_mutually_exclusive_group(required=True)
    source.add_argument("--code", help="Python code string to execute")
    source.add_argument("--file", help="Path to a .py file to execute (breakpoints work with debugpy)")

    args = parser.parse_args(argv)
    if args.command is None:
        parser.print_help()
        sys.exit(1)

    if args.command == "list":
        cmd_list(args)
    elif args.command == "exec":
        cmd_exec(args)


if __name__ == "__main__":
    main()
