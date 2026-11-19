"""Print the files of a Hub dataset at a revision that are not yet in a local directory, one per line.

    uv run --with huggingface_hub scripts/hub_missing_files.py <repo> <revision> <dir> [folder/ ...]

With folders given, only files under them are listed (e.g. `meta/ data/` to skip the videos).

Resuming with `hf download` re-checks every file already on disk with one Hub request each, which on a dataset of
tens of thousands of files costs as long as downloading them again under the per-file rate limit. Listing the repo
once and passing only the missing files avoids that.
"""

import os
import sys


def missing(remote, local_dir, folders=()):
    return [f for f in remote
            if (not folders or f.startswith(tuple(folders))) and not os.path.isfile(os.path.join(local_dir, f))]


def main():
    from huggingface_hub import HfApi  # here, so the tests run without it

    repo, revision, local_dir = sys.argv[1:4]
    remote = HfApi().list_repo_files(repo, repo_type="dataset", revision=revision)
    for f in missing(remote, local_dir, sys.argv[4:]):
        print(f)


if __name__ == "__main__":
    main()
