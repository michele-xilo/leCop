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
# The token goes in a header file, so it never shows in the process list.
hdr=$(mktemp)
trap 'rm -f "$hdr"' EXIT
token=${HF_TOKEN:-}
token_file=${HF_TOKEN_PATH:-$HOME/.cache/huggingface/token}
if [ -z "$token" ] && [ -f "$token_file" ]; then token=$(cat "$token_file"); fi
if [ -n "$token" ]; then printf 'Authorization: Bearer %s\n' "$token" >"$hdr"; fi
# curl in parallel rather than hf download: on language_table_lerobot (442k small files) hf download fetched 240 files
# a minute, 32 curls 3,300. The Hub allows 12,000 file requests per 5 minutes; --retry waits out a 429 past that.
# Each file is written under .part and renamed, so an interrupted fetch never leaves a truncated file behind.
fetch() {
  missing_files "$1" "$2" "$3" | tr '\n' '\0' |
    REPO=$1 REV=$2 DIR=$3 HDR=$hdr xargs -0 -r -n 1 -P "${FETCH_JOBS:-16}" sh -c '
      mkdir -p "$DIR/$(dirname "$1")" &&
        curl -fsSL --retry 10 --retry-all-errors -H @"$HDR" -o "$DIR/$1.part" \
          "https://huggingface.co/datasets/$REPO/resolve/$REV/$1" &&
        mv "$DIR/$1.part" "$DIR/$1"' sh
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
  # A partial download is kept and only its missing files are fetched; a second pass catches transient failures.
  echo "fetching $repo ($size GB)"
  mkdir -p "$dir"
  fetch "$repo" "$rev" "$dir" || fetch "$repo" "$rev" "$dir" || true
  left=$(missing_files "$repo" "$rev" "$dir" | wc -l)
  if [ "$left" -eq 0 ]; then touch "$dir/.complete"; else echo "FAILED   $repo ($left files missing)"; fi
done
