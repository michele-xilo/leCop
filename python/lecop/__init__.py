"""Check a LeRobot dataset from Python.

The wheel ships the `lecop` binary; `validate` runs it with `--format json` and returns the parsed report.
"""
from __future__ import annotations

import json
import os
import shutil
import subprocess
import sysconfig
from typing import Any, Dict, Optional, Sequence, Union

__all__ = ["LecopError", "validate"]


class LecopError(RuntimeError):
    """leCop produced no report: the path or an option is invalid, or the binary failed."""


def _binary() -> str:
    name = "lecop.exe" if os.name == "nt" else "lecop"
    bundled = os.path.join(sysconfig.get_path("scripts"), name)
    if os.path.exists(bundled):
        return bundled
    found = shutil.which("lecop")
    if found is None:
        raise LecopError("the lecop binary was not found; reinstall the lecop wheel")
    return found


def validate(
    path: Union[str, "os.PathLike[str]"],
    *,
    decode: bool = False,
    jobs: Optional[int] = None,
    ignore: Sequence[str] = (),
    select: Sequence[str] = (),
) -> Dict[str, Any]:
    """Validate the dataset at `path` (or every dataset under it) and return the report.

    The report is the CLI's `--format json` object: `errors` and `warnings` counts, `ignored` (issue code to count)
    and `issues`, a list of dicts with `dataset`, `severity`, `episode`, `code` and `message`. Errors in the dataset
    do not raise; read `errors`, where a dataset leCop cannot open is a `cannot_open` issue. `LecopError` is raised
    when there is no report: the path is not a directory or holds no dataset, or an option or the config is invalid.
    """
    cmd = [_binary(), "validate", "--format", "json"]
    if decode:
        cmd.append("--decode")
    if jobs is not None:
        cmd += ["-j", str(jobs)]
    if ignore:
        cmd += ["--ignore", ",".join(ignore)]
    if select:
        cmd += ["--select", ",".join(select)]
    cmd.append(os.fspath(path))
    run = subprocess.run(cmd, capture_output=True, text=True)
    # Exit code 2 still prints a report when a dataset cannot be opened; without stdout there is none.
    if run.returncode not in (0, 1, 2) or not run.stdout.strip():
        raise LecopError(run.stderr.strip() or f"lecop exited with code {run.returncode}")
    return json.loads(run.stdout)
