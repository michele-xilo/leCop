#!/usr/bin/env python3
"""Run lecop on the datasets of scripts/datasets.tsv and write one results record per dataset (standard library only).

    scripts/site_run.py                    every dataset in datasets/ that is listed in scripts/datasets.tsv
    scripts/site_run.py --only pusht       one of them
    scripts/site_run.py --check            validate every site/data/runs/*.json against site/data/schema.json

Each record is self-contained (dataset revision, leCop version and commit, machine, threads), so records made on
different machines can sit in the same folder. Wall time is the median of RUNS timed runs of `lecop validate -q`
after one warm-up run, so it is a warm-cache number.
"""

import argparse
import json
import os
import platform
import statistics
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCHEMA = os.path.join(ROOT, "site", "data", "schema.json")
RUNS_DIR = os.path.join(ROOT, "site", "data", "runs")
TYPES = {"string": str, "integer": int, "number": (int, float), "array": list, "object": dict, "null": type(None)}


def read_registry(text):
    """(name, repo, revision) of each dataset listed in scripts/datasets.tsv."""
    out = []
    for line in text.splitlines():
        if line.strip() and not line.startswith("#"):
            name, repo, revision = line.split("\t")[:3]
            out.append({"name": name, "repo": repo, "revision": revision})
    return out


def build_record(entry, info, report, samples_ms, meta):
    return {
        **entry,
        "dataset": {
            "format": info["codebase_version"],
            "episodes": info["total_episodes"],
            "frames": info["total_frames"],
            "fps": info["fps"],
            "robot": info.get("robot_type") or "unknown",
        },
        "run": {
            "wall_ms": round(statistics.median(samples_ms), 1),
            "runs": len(samples_ms),
            "cache": "warm",
            **meta,
        },
        "errors": report["errors"],
        "warnings": report["warnings"],
        # The report's `dataset` is the local path: not part of a public record.
        "issues": [
            {"severity": i["severity"], "episode": i.get("episode"), "code": i["code"], "message": i["message"]}
            for i in report["issues"]
        ],
    }


def load_schema():
    with open(SCHEMA) as f:
        return json.load(f)


def validate(value, schema, path="record"):
    """Errors of `value` against the subset of JSON Schema that schema.json uses: type, enum, required, properties, items."""
    types = schema.get("type")
    types = [types] if isinstance(types, str) else types or []
    # bool is an int in Python; a flag is not a count.
    if types and not any(isinstance(value, TYPES[t]) and not isinstance(value, bool) for t in types):
        return [f"{path}: expected {'/'.join(types)}, got {type(value).__name__}"]
    if "enum" in schema and value not in schema["enum"]:
        return [f"{path}: {value!r} not one of {schema['enum']}"]
    errors = []
    if isinstance(value, dict):
        errors += [f"{path}.{k}: missing" for k in schema.get("required", []) if k not in value]
        for k, sub in schema.get("properties", {}).items():
            if k in value:
                errors += validate(value[k], sub, f"{path}.{k}")
    if isinstance(value, list) and "items" in schema:
        for i, item in enumerate(value):
            errors += validate(item, schema["items"], f"{path}[{i}]")
    return errors


def out(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw).stdout.strip()


def machine():
    if sys.platform == "darwin":
        return out(["sysctl", "-n", "machdep.cpu.brand_string"])
    if os.path.exists("/proc/cpuinfo"):
        with open("/proc/cpuinfo") as f:
            for line in f:
                if line.startswith("model name"):
                    return line.split(":", 1)[1].strip()
    return platform.processor() or platform.machine()


def run_meta(lecop):
    commit = out(["git", "-C", ROOT, "rev-parse", "--short", "HEAD"])
    if out(["git", "-C", ROOT, "status", "--porcelain", "--untracked-files=no"]):
        commit += "-dirty"
    return {
        "threads": os.cpu_count() or 1,
        "lecop_version": out([lecop, "--version"]).split()[-1],
        "lecop_commit": commit,
        "rustc": out(["rustc", "--version"]).split()[1],
        "machine": machine(),
        "os": f"{platform.system()} {platform.release()}",
        "date": time.strftime("%Y-%m-%d", time.gmtime()),
    }


def time_ms(cmd, runs):
    """Wall time of each of `runs` runs after one warm-up run; exit codes are ignored (1 means findings)."""
    samples = []
    for i in range(runs + 1):
        start = time.perf_counter()
        subprocess.run(cmd, capture_output=True)
        if i:
            samples.append((time.perf_counter() - start) * 1000)
    return samples


def run_dataset(entry, lecop, runs, meta):
    path = os.path.join(ROOT, "datasets", entry["name"])
    with open(os.path.join(path, "meta", "info.json")) as f:
        info = json.load(f)
    report = json.loads(subprocess.run([lecop, "validate", "--format", "json", path], capture_output=True, text=True).stdout)
    return build_record(entry, info, report, time_ms([lecop, "validate", "-q", path], runs), meta)


def check():
    schema, bad = load_schema(), 0
    for name in sorted(os.listdir(RUNS_DIR)):
        with open(os.path.join(RUNS_DIR, name)) as f:
            errors = validate(json.load(f), schema)
        for e in errors:
            print(f"{name}: {e}")
        bad += bool(errors)
    print(f"{len(os.listdir(RUNS_DIR))} records, {bad} invalid")
    return 1 if bad else 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--only", action="append", default=[], help="dataset name (repeatable)")
    ap.add_argument("--runs", type=int, default=5, help="timed runs per dataset (default 5)")
    ap.add_argument("--lecop", default=os.environ.get("LECOP", os.path.join(ROOT, "target/release/lecop")))
    ap.add_argument("--check", action="store_true", help="validate the committed records and exit")
    args = ap.parse_args()
    if args.check:
        return check()
    with open(os.path.join(ROOT, "scripts", "datasets.tsv")) as f:
        entries = read_registry(f.read())
    meta = run_meta(args.lecop)
    os.makedirs(RUNS_DIR, exist_ok=True)
    for entry in entries:
        if args.only and entry["name"] not in args.only:
            continue
        if not os.path.isdir(os.path.join(ROOT, "datasets", entry["name"])):
            print(f"missing  {entry['name']}: run scripts/fetch-datasets.sh --yes {entry['name']}")
            continue
        record = run_dataset(entry, args.lecop, args.runs, meta)
        with open(os.path.join(RUNS_DIR, entry["name"] + ".json"), "w") as f:
            json.dump(record, f, indent=2)
            f.write("\n")
        print(f"{entry['name']}: {record['run']['wall_ms']} ms, {record['errors']}E {record['warnings']}W")
    return 0


if __name__ == "__main__":
    sys.exit(main())
