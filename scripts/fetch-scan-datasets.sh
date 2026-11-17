#!/bin/sh
# Download the Hub scan datasets listed in scripts/scan-datasets.tsv into datasets/scan/<owner>__<name>, one at
# a time. Without --yes it only prints what is missing. Usage: scripts/fetch-scan-datasets.sh [--yes] [repo...]
set -eu
cd "$(dirname "$0")/.."
yes=0
if [ "${1:-}" = "--yes" ]; then yes=1; shift; fi
# Stop well before the disk fills, as datasets/ may be on a disk shared with other people.
min_free_gb=${MIN_FREE_GB:-100}
# Same fallback as fetch-datasets.sh: the hf client rejects some files the Hub serves with other bytes.
curl_fetch() {
  repo=$1 rev=$2 dir=$3
  curl -fsSL "https://huggingface.co/api/datasets/$repo/tree/$rev?recursive=true" |
    python3 -c 'import json,sys; [print(f["path"]) for f in json.load(sys.stdin) if f["type"]=="file"]' |
    while read -r f; do
      mkdir -p "$dir/$(dirname "$f")"
      curl -fsSL -o "$dir/$f" "https://huggingface.co/datasets/$repo/resolve/$rev/$f"
    done
}
mkdir -p datasets/scan
grep -v '^#' scripts/scan-datasets.tsv | while IFS="$(printf '\t')" read -r repo rev _ size _; do
  if [ $# -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$repo"; then continue; fi
  dir="datasets/scan/$(echo "$repo" | sed 's#/#__#')"
  [ -f "$dir/.complete" ] && { echo "have     $repo"; continue; }
  if [ "$yes" = 0 ]; then echo "missing  $repo ($size GB)"; continue; fi
  free_gb=$(df -P -BG datasets/scan/ | awk 'NR==2 {sub("G", "", $4); print $4}')
  if [ "$free_gb" -lt "$min_free_gb" ]; then
    echo "stopping: $free_gb GB free, below $min_free_gb GB"; exit 1
  fi
  # No clean-up of a partial download: hf download resumes it, which matters for datasets of tens of thousands of
  # files that take hours at the Hub's per-file rate limit.
  echo "fetching $repo ($size GB)"
  if uvx --from huggingface_hub hf download "$repo" --repo-type dataset --revision "$rev" --local-dir "$dir" >/dev/null 2>&1 ||
     { echo "hf download failed, retrying $repo with curl"; curl_fetch "$repo" "$rev" "$dir"; }; then
    touch "$dir/.complete"
  else
    echo "FAILED   $repo"
  fi
done
