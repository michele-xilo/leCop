#!/bin/sh
# Download the real datasets listed in scripts/datasets.tsv into datasets/ (git-ignored).
# Without --yes it only prints what is missing. Usage: scripts/fetch-datasets.sh [--yes] [name...]
set -eu
cd "$(dirname "$0")/.."
yes=0
if [ "${1:-}" = "--yes" ]; then yes=1; shift; fi
# The hf client verifies sizes against the Hub listing and rejects some files the Hub serves with other
# bytes; per-file curl does not.
curl_fetch() {
  repo=$1 rev=$2 dir=$3
  curl -fsSL "https://huggingface.co/api/datasets/$repo/tree/$rev?recursive=true" |
    python3 -c 'import json,sys; [print(f["path"]) for f in json.load(sys.stdin) if f["type"]=="file"]' |
    while read -r f; do
      mkdir -p "$dir/$(dirname "$f")"
      curl -fsSL -o "$dir/$f" "https://huggingface.co/datasets/$repo/resolve/$rev/$f"
    done
}
grep -v '^#' scripts/datasets.tsv | while IFS="$(printf '\t')" read -r name repo rev _ _; do
  if [ $# -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$name"; then continue; fi
  [ -d "datasets/$name" ] && { echo "have     $name"; continue; }
  if [ "$yes" = 1 ]; then
    echo "fetching $name ($repo @ ${rev%"${rev#????????}"})"
    uvx --from huggingface_hub hf download "$repo" --repo-type dataset --revision "$rev" --local-dir "datasets/$name" >/dev/null ||
      { echo "hf download failed, retrying $name with curl"; curl_fetch "$repo" "$rev" "datasets/$name"; }
  else
    echo "missing  $name ($repo); rerun with --yes to download"
  fi
done
