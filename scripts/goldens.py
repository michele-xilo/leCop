#!/usr/bin/env python3
"""Record what other dataset checkers report, as small normalized goldens for tests/goldens.rs.

For every dataset in scripts/datasets.tsv that is present under datasets/, run lerobot-doctor and physlint and write
tests/goldens/<dataset>.json: finding kinds, episode indices and severities only, never dataset contents.

    LERO_DOCTOR=path/to/lerobot-doctor PHYSLINT=path/to/physlint python3 scripts/goldens.py

Install the tools into a throwaway venv (`uv venv; uv pip install lerobot-doctor physlint`), never into the repo.
Each tool's finding kinds map to lecop issue codes in scripts/golden_map.tsv.
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (regex on a lerobot-doctor message, kind). The first match wins; kinds are the keys of golden_map.tsv.
DOCTOR_KINDS = [
    (r"_is_pad|source_fps|^next\.", "auxiliary_column"),
    (r"^\.\.\.and \d+ more|episode\(s\) flagged$", None),
    (r"consecutive identical|frozen", "action_frozen"),
    (r"sudden large action jumps|action jump\(s\)", "action_jump"),
    (r"distribution shift", "distribution_shift"),
    (r"stuck/static", "stuck_in_some_episodes"),
    (r"ALL \d+ dimensions constant", "all_dimensions_constant"),
    (r"zero variance", "constant_dimension"),
    (r"% of values at (minimum|maximum)", "clipped_dimension"),
    (r"extreme outlier", "outlier_values"),
    (r"length mismatch", "episode_length_mismatch"),
    (r"abnormally long episode", "long_episode"),
    (r"shorter than chunk_size", "shorter_than_chunk_size"),
    (r"total_frames=\d+ but actual", "total_frames_mismatch"),
    (r"Global index column is not sequential", "index_not_sequential"),
    (r"Missing episode indices", "missing_episode_indices"),
    (r"No stats\.json -- normalization", "no_stats_for_training"),
    (r"stats\.json not found", "stats_json_missing"),
    (r"has \d+ frames", "video_frame_count"),
]


def episodes_in(message):
    """Episode indices a lerobot-doctor message names, or [None] for a dataset-wide finding."""
    m = re.search(r"[Ee]pisode (\d+)", message)
    if m:
        return [int(m.group(1))]
    m = re.search(r"abnormally long episode\(s\).*: \[([\d, ]+)\]", message)
    if m:
        return [int(x) for x in m.group(1).split(",")]
    m = re.search(r"Missing episode indices: \[([\d, ]+)", message)
    if m:
        return [None]
    m = re.search(r"episode_(\d+)\.mp4", message)
    if m:
        return [int(m.group(1))]
    m = re.search(r"\[\((\d+), 'data=", message)
    if m:
        return [int(x) for x in re.findall(r"\((\d+), 'data=", message)]
    return [None]


def run(cmd):
    out = subprocess.run(cmd, capture_output=True, text=True)
    return json.loads(out.stdout) if out.stdout.strip() else None


def doctor(tool, path):
    d = run([tool, "check", "--json", str(path)])
    if d is None:
        return None
    version = subprocess.run([tool, "--version"], capture_output=True, text=True).stdout.split()[-1]
    findings = set()
    for check in d["checks"]:
        for m in check["messages"]:
            if m["severity"] not in ("WARN", "FAIL"):
                continue
            kind = next((k for pat, k in DOCTOR_KINDS if re.search(pat, m["message"])), "unclassified")
            if kind is None:
                continue
            # lerobot-doctor warns about a missing stats.json on v2.1, which keeps its statistics in episodes_stats.jsonl.
            if kind == "stats_json_missing" and d["codebase_version"].startswith("v2.1"):
                kind = "stats_json_missing_v2_1"
            for ep in episodes_in(m["message"]):
                findings.add((kind, ep, m["severity"].lower()))
    return {"version": version, "findings": sorted(findings, key=str)}


def physlint(tool, path):
    d = run([tool, "check", "-f", "json", str(path)])
    if d is None:
        return None
    findings = set()
    for r in d["results"]:
        for f in r["findings"]:
            ep = f["location"].get("episode")
            ep = int(re.sub(r"\D", "", ep)) if ep else None
            findings.add((r["rule_id"], ep, f["severity"]))
    return {"version": d["physlint_version"], "findings": sorted(findings, key=str)}


def main():
    tools = {"lerobot-doctor": (os.environ.get("LERO_DOCTOR"), doctor), "physlint": (os.environ.get("PHYSLINT"), physlint)}
    out_dir = ROOT / "tests" / "goldens"
    out_dir.mkdir(exist_ok=True)
    for line in (ROOT / "scripts" / "datasets.tsv").read_text().splitlines():
        if line.startswith("#") or not line.strip():
            continue
        name = line.split("\t")[0]
        path = ROOT / "datasets" / name
        if not path.exists():
            print(f"skipping {name}: not downloaded", file=sys.stderr)
            continue
        golden = {"dataset": name, "tools": {}}
        for tool_name, (exe, fn) in tools.items():
            if not exe:
                print(f"{tool_name}: set its path in the environment", file=sys.stderr)
                continue
            r = fn(exe, path)
            if r is None:
                print(f"{name}: {tool_name} gave no output", file=sys.stderr)
                continue
            golden["tools"][tool_name] = {
                "version": r["version"],
                "findings": [{"kind": k, "episode": e, "severity": s} for k, e, s in r["findings"]],
            }
        (out_dir / f"{name}.json").write_text(json.dumps(golden, indent=1) + "\n")
        print(name, {t: len(v["findings"]) for t, v in golden["tools"].items()})


if __name__ == "__main__":
    main()
