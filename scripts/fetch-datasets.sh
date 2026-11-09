#!/bin/sh
# Download the real datasets listed in scripts/datasets.tsv into datasets/ (git-ignored).
# Without --yes it only prints what is missing. Usage: scripts/fetch-datasets.sh [--yes] [name...]
set -eu
cd "$(dirname "$0")/.."
yes=0
if [ "${1:-}" = "--yes" ]; then yes=1; shift; fi
grep -v '^#' scripts/datasets.tsv | while IFS="$(printf '\t')" read -r name repo rev _ _; do
  if [ $# -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$name"; then continue; fi
  [ -d "datasets/$name" ] && { echo "have     $name"; continue; }
  if [ "$yes" = 1 ]; then
    echo "fetching $name ($repo @ ${rev%"${rev#????????}"})"
    uvx --from huggingface_hub hf download "$repo" --repo-type dataset --revision "$rev" --local-dir "datasets/$name" >/dev/null
  else
    echo "missing  $name ($repo); rerun with --yes to download"
  fi
done
