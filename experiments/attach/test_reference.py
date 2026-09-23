"""Opt-in smoke checks for the attached host processes owned by these probes."""

import argparse
import os
from pathlib import Path

import pytest

from probe import _bootstrap, add_host_arguments, run_probe
from bridge_spike import run_spike


def _arguments(host, output):
    parser = argparse.ArgumentParser()
    add_host_arguments(parser)
    return parser.parse_args([
        "--host", host, "--exe", os.environ["FLINT_ATTACH_EXE"],
        "--injector", os.environ["FLINT_ATTACH_INJECTOR"], "--out", str(output),
        "--startup-wait", os.environ.get("FLINT_ATTACH_STARTUP_WAIT", "60"),
        "--blender-dispatch", os.environ.get("FLINT_ATTACH_BLENDER_DISPATCH", "pending"),
    ])


def test_bootstrap_handles_paths_with_quotes(tmp_path):
    path = tmp_path / "user's project" / "payload.py"
    bootstrap = _bootstrap(path)
    assert "'" not in bootstrap
    assert len(bootstrap.encode("utf-8")) < 2047
    compile(bootstrap, "<injected bootstrap>", "exec")


@pytest.mark.skipif(not os.environ.get("FLINT_ATTACH_HOST"), reason="Set FLINT_ATTACH_HOST, EXE, and INJECTOR")
def test_new_host_main_thread_probe(tmp_path):
    args = _arguments(os.environ["FLINT_ATTACH_HOST"], tmp_path / "probe")
    result = run_probe(args)
    assert result["main_thread"]


@pytest.mark.skipif(not os.environ.get("FLINT_ATTACH_SPIKE_HOST"), reason="Set FLINT_ATTACH_SPIKE_HOST, EXE, INJECTOR, and BINARY")
def test_new_host_bridge_roundtrip(tmp_path):
    args = _arguments(os.environ["FLINT_ATTACH_SPIKE_HOST"], tmp_path / "spike")
    args.flint_exe = Path(os.environ["FLINT_ATTACH_FLINT_EXE"])
    run_spike(args)
    assert (args.out / "execution.json").is_file()
