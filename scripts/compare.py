#!/usr/bin/env python3
"""Wall-time comparison of lecop against other LeRobot dataset checkers on the same datasets, as markdown tables.

The tools do different amounts of work on video, so two tables are printed (MODE=both, the default):

  no video decoding   lerobot-doctor without its `videos` check, physlint with its three `video.*` rules off,
                      lecop as built (container metadata only).
  video decoding      physlint with all rules (it decodes every frame) and lecop built with `--features decode`
                      and run with `--decode`, on the v3 datasets physlint reads. lerobot-doctor is left out: it
                      decodes one frame per video, which is not the same work.

Every run starts in a fresh empty working directory and physlint's result cache is off, so no tool is served a
previous result. Each tool runs RUNS times per dataset; the first run is a warm-up (page cache), the median of
the rest is reported. Wall time includes process start. The tools check different things (see docs/comparison.md), so
this compares speed only.

Install the others into throwaway venvs (not the repo), for example:
    uv venv doctor-env   && uv pip install --python doctor-env/bin/python lerobot-doctor==0.2.0
    uv venv physlint-env && uv pip install --python physlint-env/bin/python "physlint[video]==0.4.0"
and point at them with LERO_DOCTOR=/path/to/lerobot-doctor and PHYSLINT=/path/to/physlint. The decoding table
also needs LECOP_DECODE=/path/to/lecop built with `cargo build --release --features decode` (use another
CARGO_TARGET_DIR so it does not replace target/release/lecop).
Usage: scripts/compare.py datasets/pusht datasets/v3_pusht ...   (RUNS=5, MODE=novideo|video|both)
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
MODE = os.environ.get("MODE", "both")
LECOP = os.environ.get("LECOP", os.path.join(ROOT, "target/release/lecop"))
LECOP_DECODE = os.environ.get("LECOP_DECODE")
# lerobot-doctor 0.2.0 check names, `videos` left out for the no-decoding table.
DOCTOR_CHECKS_WITHOUT_VIDEO = (
    "metadata,temporal,actions,statistics,episodes,consistency,training,anomalies,portability,per_episode"
)
PHYSLINT_VIDEO_RULES = ("video.decode", "video.frozen_frames", "video.black_frames")


def version(ds):
    with open(os.path.join(ds, "meta/info.json")) as f:
        return json.load(f)["codebase_version"]


def physlint_config(path, decode):
    rules = "" if decode else "rules:\n" + "".join(f"  {r}:\n    enabled: false\n" for r in PHYSLINT_VIDEO_RULES)
    with open(path, "w") as f:
        f.write(
            "config_version: 1\nadapter: auto\nprofile: auto\nrequired_streams:\n  - observation.state\n"
            f"  - action\nfail_on: error\n{rules}cache:\n  enabled: false\n"
        )


def tools(ds, decode, config_dir):
    """(name, command) for every tool that can read `ds` in this mode."""
    if decode:
        lecop = [LECOP_DECODE, "validate", "-q", "--decode", ds] if LECOP_DECODE else None
    else:
        lecop = [LECOP, "validate", "-q", ds]
    out = [("leCop", lecop)] if lecop else []
    doctor = os.environ.get("LERO_DOCTOR") or shutil.which("lerobot-doctor")
    if doctor and not decode:
        cmd = [doctor, "check", ds] + ([] if decode else ["--checks", DOCTOR_CHECKS_WITHOUT_VIDEO])
        out.append(("lerobot-doctor", cmd))
    physlint = os.environ.get("PHYSLINT") or shutil.which("physlint")
    if physlint and version(ds).startswith("v3"):  # physlint reads v3 only
        config = os.path.join(config_dir, "decode.yaml" if decode else "novideo.yaml")
        out.append(("physlint", [physlint, "check", ds, "--config", config]))
    return out


def median_ms(cmd):
    times = []
    for i in range(RUNS + 1):
        with tempfile.TemporaryDirectory() as cwd:
            t = time.perf_counter()
            subprocess.run(cmd, cwd=cwd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            if i:
                times.append((time.perf_counter() - t) * 1e3)
    return statistics.median(times)


def ms(x):
    return f"{x:,.0f} ms"


def row(ds, ms_by_tool, cols, vs):
    """Markdown row: the fastest tool gets the trophy; the last column is leCop against the fastest tool in `vs`."""
    name = f"`{os.path.basename(ds)}` ({version(ds)})"
    fastest = min(ms_by_tool, key=ms_by_tool.get)
    cells = []
    for tool in cols:
        if tool not in ms_by_tool:
            cells.append("n/a")
        elif tool == fastest:
            cells.append(f"🏆 **{ms(ms_by_tool[tool])}**")
        else:
            cells.append(ms(ms_by_tool[tool]))
    others = [v for k, v in ms_by_tool.items() if k in vs]
    ratio = "n/a"
    if others and "leCop" in ms_by_tool:
        r = min(others) / ms_by_tool["leCop"]
        ratio = f"{r:.0f}x" if r >= 10 else f"{r:.2g}x"
    return f"| {name} | " + " | ".join(cells) + f" | {ratio} |"


def table(datasets, decode, config_dir):
    title = "video decoding" if decode else "no video decoding"
    print(f"\n{title}: median wall time over {RUNS} runs after one warm-up, fresh directory per run\n")
    cols = ("leCop", "physlint") if decode else ("leCop", "lerobot-doctor", "physlint")
    vs = cols[1:]
    print(f"| Dataset | {' | '.join(cols)} | leCop vs {' or '.join(vs)} |")
    print("|---" * (len(cols) + 2) + "|")
    for ds in datasets:
        if decode and not version(ds).startswith("v3"):  # physlint reads v3 only
            continue
        times = {name: median_ms(cmd) for name, cmd in tools(ds, decode, config_dir)}
        print(row(ds, times, cols, vs), flush=True)


def main():
    datasets = [os.path.abspath(d) for d in sys.argv[1:]]
    if not datasets:
        sys.exit(__doc__)
    with tempfile.TemporaryDirectory() as config_dir:
        physlint_config(os.path.join(config_dir, "novideo.yaml"), decode=False)
        physlint_config(os.path.join(config_dir, "decode.yaml"), decode=True)
        if MODE in ("novideo", "both"):
            table(datasets, False, config_dir)
        if MODE in ("video", "both"):
            if not LECOP_DECODE:
                print("\nvideo decoding: skipped, set LECOP_DECODE to a lecop built with --features decode")
            else:
                table(datasets, True, config_dir)


main()
