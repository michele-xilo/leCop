#!/usr/bin/env python3
"""Run lecop on the datasets of scripts/datasets.tsv and write one results record per dataset (standard library only).

    scripts/site_run.py                    every dataset in datasets/ that is listed in scripts/datasets.tsv
    scripts/site_run.py --only pusht       one of them
    scripts/site_run.py --scan             every downloaded dataset of scripts/scan-datasets.tsv (datasets/scan/)
    scripts/site_run.py --scan --decode    the scan's decode=yes datasets with --decode, into <name>.decode.json
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
TYPES = {"boolean": bool, "string": str, "integer": int, "number": (int, float), "array": list, "object": dict, "null": type(None)}


def read_registry(text):
    """(name, repo, revision) of each dataset listed in scripts/datasets.tsv."""
    out = []
    for line in text.splitlines():
        if line.strip() and not line.startswith("#"):
            name, repo, revision = line.split("\t")[:3]
            out.append({"name": name, "repo": repo, "revision": revision})
    return out


def read_scan_list(text, decode_only=False):
    """(name, repo, revision) of each dataset in scripts/scan-datasets.tsv, named owner__name as on disk.
    decode_only keeps the rows whose last column is `yes`."""
    out = []
    for line in text.splitlines():
        if line.strip() and not line.startswith("#"):
            cols = line.split("\t")
            if decode_only and cols[-1].strip() != "yes":
                continue
            out.append({"name": cols[0].replace("/", "__"), "repo": cols[0], "revision": cols[1]})
    return out


def record_file(name, decode):
    """A --decode run is a second record next to the default one, not a replacement for it."""
    return f"{name}.decode.json" if decode else f"{name}.json"


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
        # The report's `dataset` is the local path: not part of a public record, nor are messages that quote it.
        "issues": [
            {"severity": i["severity"], "episode": i.get("episode"), "code": i["code"], "message": relative(i["message"], i["dataset"])}
            for i in report["issues"]
        ],
    }


def relative(message, root):
    return message.replace(root + "/", "").replace(root, ".")


def load_schema():
    with open(SCHEMA) as f:
        return json.load(f)


def validate(value, schema, path="record"):
    """Errors of `value` against the subset of JSON Schema that schema.json uses: type, enum, required, properties, items."""
    types = schema.get("type")
    types = [types] if isinstance(types, str) else types or []
    # bool is an int in Python; a flag is not a count.
    if types and not any(isinstance(value, TYPES[t]) and (t == "boolean" or not isinstance(value, bool)) for t in types):
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


def run_dataset(entry, path, lecop, runs, meta):
    with open(os.path.join(path, "meta", "info.json")) as f:
        info = json.load(f)
    flags = ["--decode"] if meta.get("decode") else []
    proc = subprocess.run([lecop, "validate", "--format", "json", *flags, path], capture_output=True, text=True)
    try:
        report = json.loads(proc.stdout)
    except json.JSONDecodeError:
        # A crash gets no record: it is a P0 bug to file, not a result to publish.
        raise RuntimeError(f"lecop exited {proc.returncode} without a JSON report: {proc.stderr.strip()[-300:]}")
    return build_record(entry, info, report, time_ms([lecop, "validate", "-q", *flags, path], runs), meta)


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
    ap.add_argument("--scan", action="store_true", help="the Hub scan list, scripts/scan-datasets.tsv, in datasets/scan/")
    ap.add_argument("--decode", action="store_true", help="run with --decode into <name>.decode.json (with --scan: the decode=yes rows)")
    args = ap.parse_args()
    if args.check:
        return check()
    if args.scan:
        with open(os.path.join(ROOT, "scripts", "scan-datasets.tsv")) as f:
            entries = read_scan_list(f.read(), decode_only=args.decode)
        base, fetch, ready = os.path.join(ROOT, "datasets", "scan"), "scripts/fetch-scan-datasets.sh --yes", ".complete"
    else:
        with open(os.path.join(ROOT, "scripts", "datasets.tsv")) as f:
            entries = read_registry(f.read())
        base, fetch, ready = os.path.join(ROOT, "datasets"), "scripts/fetch-datasets.sh --yes", ""
    meta = {**run_meta(args.lecop), "decode": args.decode}
    os.makedirs(RUNS_DIR, exist_ok=True)
    crashed = 0
    for entry in entries:
        if args.only and entry["name"] not in args.only:
            continue
        path = os.path.join(base, entry["name"])
        if not os.path.exists(os.path.join(path, ready)):
            print(f"missing  {entry['name']}: run {fetch} {entry['repo'] if args.scan else entry['name']}")
            continue
        try:
            record = run_dataset(entry, path, args.lecop, args.runs, meta)
        except RuntimeError as e:
            print(f"CRASH    {entry['name']}: {e}")
            crashed += 1
            continue
        with open(os.path.join(RUNS_DIR, record_file(entry["name"], args.decode)), "w") as f:
            json.dump(record, f, indent=2)
            f.write("\n")
        print(f"{entry['name']}: {record['run']['wall_ms']} ms, {record['errors']}E {record['warnings']}W")
    return 1 if crashed else 0


if __name__ == "__main__":
    sys.exit(main())
