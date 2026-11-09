#!/usr/bin/env python3
"""Wall-time comparison of lecop against other LeRobot dataset checkers on the same datasets.

Each tool is run RUNS times per dataset (first run is a warm-up, so the page cache is warm for all tools) and
the median is printed. Wall time includes process start. The tools check different things, so this compares
speed only, not coverage.

Install the others into throwaway venvs (not the repo), for example:
    uv venv doctor-env   && uv pip install --python doctor-env/bin/python lerobot-doctor==0.2.0
    uv venv physlint-env && uv pip install --python physlint-env/bin/python "physlint[video]==0.4.0"
and point at them with LERO_DOCTOR=/path/to/lerobot-doctor and PHYSLINT=/path/to/physlint.
Usage: scripts/compare.py datasets/pusht datasets/v3_pusht ...   (RUNS=5 to change the repeat count)
"""

import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RUNS = int(os.environ.get("RUNS", "3"))
LECOP = os.environ.get("LECOP", os.path.join(ROOT, "target/release/lecop"))


def version(ds):
    with open(os.path.join(ds, "meta/info.json")) as f:
        return json.load(f)["codebase_version"]


def tools(ds):
    out = [("lecop", [LECOP, "validate", "-q", ds])]
    doctor = os.environ.get("LERO_DOCTOR") or shutil.which("lerobot-doctor")
    if doctor:
        out.append(("lerobot-doctor", [doctor, "check", ds]))
    physlint = os.environ.get("PHYSLINT") or shutil.which("physlint")
    # physlint reads v3 only
    if physlint and version(ds).startswith("v3"):
        out.append(("physlint", [physlint, "check", ds]))
    return out


def median_ms(cmd, cwd):
    times = []
    for i in range(RUNS + 1):
        t = time.perf_counter()
        subprocess.run(cmd, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if i:
            times.append((time.perf_counter() - t) * 1e3)
    return statistics.median(times)


def main():
    datasets = [os.path.abspath(d) for d in sys.argv[1:]]
    if not datasets:
        sys.exit(__doc__)
    # physlint writes a .physlint/ report directory into the working directory
    with tempfile.TemporaryDirectory() as cwd:
        print(f"median wall time over {RUNS} runs, warm cache (ms)")
        for ds in datasets:
            cells = [f"{n} {median_ms(c, cwd):.0f}" for n, c in tools(ds)]
            print(f"{os.path.basename(ds):<40} {version(ds):<5} " + "   ".join(cells))


main()
