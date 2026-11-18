"""Write fits.json: which of the Space's listed datasets fit its download caps, for the dropdown's order.

    uv run --with huggingface_hub==2.4.0 python space/build_fits.py

Runs with your local Hugging Face login when there is one: the anonymous rate limit makes the few thousand requests
take hours. Re-run it before uploading the Space when the list or the caps change. It is not uploaded to the Space.
"""

from __future__ import annotations

import datetime
import json
import os
import sys
import time
from concurrent.futures import ThreadPoolExecutor

from huggingface_hub import HfApi
from huggingface_hub.utils import HfHubHTTPError

from hub import CAP_BYTES, CAP_FILES, list_folder, listed_datasets, site_index
from lecop_space import fit_status

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fits.json")


def main() -> None:
    api = HfApi()
    repos = listed_datasets(api, site_index())
    status = {}

    def one(repo: str) -> None:
        try:
            status[repo] = fit_status(lambda folder: list_folder(api, repo, "main", folder), CAP_BYTES, CAP_FILES)
        except HfHubHTTPError as e:
            print(f"{repo}: {e}", file=sys.stderr)

    start = time.time()
    with ThreadPoolExecutor(8) as pool:
        list(pool.map(one, repos))
    counts = {}
    for s in status.values():
        counts[s] = counts.get(s, 0) + 1
    print(f"{len(status):,} of {len(repos):,} classified in {time.time() - start:.0f} s: {counts}")
    with open(OUT, "w") as f:
        json.dump(
            {
                "built": datetime.date.today().isoformat(),
                "cap_bytes": CAP_BYTES,
                "cap_files": CAP_FILES,
                "status": dict(sorted(status.items())),
            },
            f,
            indent=0,
        )
        f.write("\n")


if __name__ == "__main__":
    main()
