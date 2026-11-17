"""Print the files of a Hub dataset at a revision that are not yet in a local directory, one per line.

    uv run --with huggingface_hub scripts/hub_missing_files.py <repo> <revision> <dir>

Resuming with `hf download` re-checks every file already on disk with one Hub request each, which on a dataset of
tens of thousands of files costs as long as downloading them again under the per-file rate limit. Listing the repo
once and passing only the missing files avoids that.
"""

import os
import sys

from huggingface_hub import HfApi


def missing(remote, local_dir):
    return [f for f in remote if not os.path.isfile(os.path.join(local_dir, f))]


def main():
    repo, revision, local_dir = sys.argv[1:4]
    remote = HfApi().list_repo_files(repo, repo_type="dataset", revision=revision)
    for f in missing(remote, local_dir):
        print(f)


if __name__ == "__main__":
    main()
