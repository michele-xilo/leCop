"""The Space's Hub queries, shared by the app and by `build_fits.py`. No Gradio here."""

from __future__ import annotations

import json
import os
import urllib.request
from typing import Iterator, List, Optional, Tuple

from huggingface_hub import HfApi
from huggingface_hub.hf_api import RepoFile
from huggingface_hub.utils import EntryNotFoundError, HfHubHTTPError

from lecop_space import SITE_URL, dataset_choices

CAP_BYTES = int(os.environ.get("LECOP_SPACE_MAX_BYTES", 2_000_000_000))
# Anonymous downloads from the Hub run at about 13 files/s whether 16, 32 or 64 at a time (lerobot/pusht v2.1, 210
# files: 16.3, 16.8 and 15.8 s), so 2,000 files is a wait of about 2.5 minutes.
CAP_FILES = int(os.environ.get("LECOP_SPACE_MAX_FILES", 2_000))
LISTED = 2000


def site_index() -> list:
    """The results site's list of datasets, or an empty list when the site cannot be reached."""
    try:
        with urllib.request.urlopen(f"{SITE_URL}/index.json", timeout=5) as r:
            return json.load(r)
    except (OSError, ValueError):
        return []


def lerobot_datasets(api: HfApi, limit: int) -> List[str]:
    """Public, ungated datasets the Hub tags as LeRobot, most downloaded first."""
    try:
        found = api.list_datasets(filter="LeRobot", sort="downloads", limit=limit, expand=["gated", "private"])
        return [d.id for d in found if not d.private and not d.gated]
    except HfHubHTTPError:
        return []


def listed_datasets(api: HfApi, index: list) -> List[str]:
    """The dropdown's datasets: the most downloaded LeRobot datasets, then the results site's."""
    return dataset_choices(lerobot_datasets(api, LISTED), index)


def list_folder(api: HfApi, repo: str, revision: str, folder: str) -> Iterator[Tuple[str, Optional[int]]]:
    """(path, size) of every file under `folder`; nothing when the folder does not exist."""
    try:
        for entry in api.list_repo_tree(repo, path_in_repo=folder, recursive=True, revision=revision, repo_type="dataset"):
            if isinstance(entry, RepoFile):
                yield entry.path, entry.size
    except EntryNotFoundError:
        return
