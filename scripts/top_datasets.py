"""Print the N most downloaded LeRobot datasets on the Hub as rows of scripts/top-datasets.tsv, with no size filter.

    uv run --with huggingface_hub scripts/top_datasets.py 50 > scripts/top-datasets.tsv

Gated datasets and repositories without meta/info.json are listed as comments with the reason, so the list says
what was left out. `meta_gb` is the size of meta/ and data/, the only folders leCop reads without videos, so it is
what the fetch costs.
"""

import json
import sys

from huggingface_hub import HfApi, hf_hub_download

HEADER = "# repo\trevision\tversion\ttotal_gb\tmeta_gb\tfiles_meta_data\tepisodes\tdownloads_30d"


def row(api, d):
    files = [f for f in api.list_repo_tree(d.id, repo_type="dataset", revision=d.sha, recursive=True)
             if getattr(f, "size", None) is not None]
    total = sum(f.size for f in files)
    kept = [f for f in files if f.path.startswith(("meta/", "data/"))]
    try:
        with open(hf_hub_download(d.id, "meta/info.json", repo_type="dataset", revision=d.sha)) as f:
            info = json.load(f)
    except Exception as e:  # not a LeRobot layout, or unreadable: listed, not fetched
        return f"# skipped {d.id}: no readable meta/info.json ({type(e).__name__})"
    return "\t".join(str(x) for x in [
        d.id, d.sha, info.get("codebase_version", "?"), round(total / 1e9, 2),
        round(sum(f.size for f in kept) / 1e9, 2), len(kept), info.get("total_episodes", "?"), d.downloads,
    ])


def main():
    n = int(sys.argv[1])
    api = HfApi()
    print(HEADER)
    taken = 0
    for d in api.list_datasets(filter="LeRobot", sort="downloads", limit=n * 2, expand=["downloads", "gated", "sha"]):
        if taken == n:
            break
        if d.gated:
            print(f"# skipped {d.id}: gated ({d.gated})", flush=True)
            continue
        line = row(api, d)
        print(line, flush=True)
        taken += not line.startswith("#")


if __name__ == "__main__":
    main()
