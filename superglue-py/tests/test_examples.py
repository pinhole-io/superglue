"""Run every examples/*.py script (tests execute the examples)."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import pytest

EXAMPLES_DIR = Path(__file__).resolve().parent.parent / "examples"
SKIP_MARKERS = (
    "Set OPENAI_API_KEY",
    "OPENAI_API_KEY is not set",
    "OPENAI_API_KEY not set",
)


def _example_python() -> str:
    """Interpreter that has the installed superglue wheel.

    ``uv run pytest`` selects ``/usr/bin/pytest`` when the project venv has no
    pytest install. That process reports ``sys.executable`` as system Python.
    ``uv run`` still sets ``VIRTUAL_ENV`` to the project venv.
    """
    venv = os.environ.get("VIRTUAL_ENV")
    if venv:
        candidate = Path(venv) / "bin" / "python"
        if candidate.is_file():
            return str(candidate)
    local = EXAMPLES_DIR.parent / ".venv" / "bin" / "python"
    if local.is_file():
        return str(local)
    return sys.executable


def _run_example(script: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [_example_python(), str(script)],
        cwd=EXAMPLES_DIR.parent,
        capture_output=True,
        text=True,
        check=False,
    )


def _example_ok(script: Path, result: subprocess.CompletedProcess[str]) -> bool:
    if result.returncode == 0:
        return True
    if os.environ.get("OPENAI_API_KEY"):
        return False
    combined = f"{result.stdout}\n{result.stderr}"
    return result.returncode == 1 and any(m in combined for m in SKIP_MARKERS)


def test_all_examples_run() -> None:
    scripts = sorted(EXAMPLES_DIR.glob("*.py"))
    assert scripts, f"no examples found in {EXAMPLES_DIR}"
    for script in scripts:
        result = _run_example(script)
        assert _example_ok(script, result), (
            f"{script.name} failed (exit {result.returncode}):\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )


@pytest.mark.live
def test_all_examples_run_live() -> None:
    if not os.environ.get("OPENAI_API_KEY"):
        pytest.skip("OPENAI_API_KEY not set")
    scripts = sorted(EXAMPLES_DIR.glob("*.py"))
    for script in scripts:
        result = _run_example(script)
        assert result.returncode == 0, (
            f"{script.name} failed (exit {result.returncode}):\n"
            f"stdout:\n{result.stdout}\nstderr:\n{result.stderr}"
        )
