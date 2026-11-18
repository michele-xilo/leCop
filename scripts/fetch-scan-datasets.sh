#!/bin/sh
# Download the Hub scan datasets listed in scripts/scan-datasets.tsv into datasets/scan/<owner>__<name>, one at
# a time. Without --yes it only prints what is missing. Usage: scripts/fetch-scan-datasets.sh [--yes] [repo...]
# For another list: LIST=scripts/top-datasets.tsv DEST=datasets/top ONLY="meta/ data/" SIZE_COL=5, where ONLY
# limits the fetch to those folders and SIZE_COL is the list's column with the size that fetch costs, in GB.
set -eu
cd "$(dirname "$0")/.."
list=${LIST:-scripts/scan-datasets.tsv} dest=${DEST:-datasets/scan} size_col=${SIZE_COL:-4}
yes=0
if [ "${1:-}" = "--yes" ]; then yes=1; shift; fi
# Stop well before the disk fills, as datasets/ may be on a disk shared with other people.
min_free_gb=${MIN_FREE_GB:-100}
# Files of the dataset not yet on disk (see hub_missing_files.py for why not a plain resume).
missing_files() {
  uv run -q --no-project --with huggingface_hub python3 scripts/hub_missing_files.py "$1" "$2" "$3" ${ONLY:-} </dev/null
}
# Same fallback as fetch-datasets.sh: the hf client rejects some files the Hub serves with other bytes.
curl_fetch() {
  repo=$1 rev=$2 dir=$3
  missing_files "$repo" "$rev" "$dir" | while read -r f; do
    mkdir -p "$dir/$(dirname "$f")"
    curl -fsSL -o "$dir/$f" "https://huggingface.co/datasets/$repo/resolve/$rev/$f"
  done
}
mkdir -p "$dest"
grep -v '^#' "$list" | cut -f "1,2,$size_col" | while IFS="$(printf '\t')" read -r repo rev size; do
  if [ $# -gt 0 ] && ! printf '%s\n' "$@" | grep -qx "$repo"; then continue; fi
  dir="$dest/$(echo "$repo" | sed 's#/#__#')"
  [ -f "$dir/.complete" ] && { echo "have     $repo"; continue; }
  if [ "$yes" = 0 ]; then echo "missing  $repo ($size GB)"; continue; fi
  free_gb=$(df -P -BG "$dest/" | awk 'NR==2 {sub("G", "", $4); print $4}')
  if [ "$free_gb" -lt "$min_free_gb" ]; then
    echo "stopping: $free_gb GB free, below $min_free_gb GB"; exit 1
  fi
  # A partial download is kept and only its missing files are fetched, in batches that fit on a command line.
  echo "fetching $repo ($size GB)"
  mkdir -p "$dir"
  missing_files "$repo" "$rev" "$dir" | tr '\n' '\0' |
    xargs -0 -r -n 1000 uvx --from huggingface_hub hf download "$repo" --repo-type dataset --revision "$rev" \
      --max-workers "${HF_MAX_WORKERS:-32}" \
      --local-dir "$dir" >/dev/null 2>&1 || true
  if [ -n "$(missing_files "$repo" "$rev" "$dir")" ]; then
    echo "hf download left files missing, retrying $repo with curl"
    curl_fetch "$repo" "$rev" "$dir" || true
  fi
  left=$(missing_files "$repo" "$rev" "$dir" | wc -l)
  if [ "$left" -eq 0 ]; then touch "$dir/.complete"; else echo "FAILED   $repo ($left files missing)"; fi
done
