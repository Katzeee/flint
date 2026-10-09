"""Compile and run submitted Python in a namespace kept across requests."""

import sys
from contextlib import redirect_stderr, redirect_stdout
from .output import ThreadScopedTextProxy


class CodeExecutor:
    def __init__(self, ns=None):
        self._ns = ns if ns is not None else {}

    def prepare(self, request):
        return compile(request["code"], request.get("filename") or "<string>", "exec")

    def run(self, prepared_result, out, err):
        stdout = ThreadScopedTextProxy(sys.stdout, out)
        stderr = ThreadScopedTextProxy(sys.stderr, err)
        with redirect_stdout(stdout), redirect_stderr(stderr):
            exec(prepared_result, self._ns, self._ns)
