# Plan: `lecop`

**One line:** the police of your robot dataset. Unofficial; for LeRobot-format datasets. (`fsck`-style checker.) One static binary that checks a LeRobot dataset
in seconds, including the video, without decoding frames.

## Why this, why us
Python validators exist (lerobot-doctor, physlint, rdq...) and have little traction. We do not
compete on "more checks". We compete on three things they structurally cannot match:

1. **Speed**: parallel (rayon), metadata-only video checks. TB-scale datasets in seconds/minutes.
2. **Video-without-decode**: read the mp4 sample tables (`moov`) only. Compare video frame count,
   duration and presentation timestamps against parquet timestamps. No ffmpeg, no GPU.
   Video/data desync is a common silent failure; decode-based tools take minutes per episode.
3. **Zero setup**: `cargo install` / prebuilt binary, no Python env. Fits CI and Hugging Face
   Space-free workflows. Machine-readable output (JSON, GitHub annotations) from day one.

## First scope (v0.1, "useful in a PR")
In:
- LeRobot **v2.1 and v3.0** (v3 is where physlint is the only competitor).
- Checks: current metadata/file/timestamp/NaN checks, **plus** per-episode video container checks
  (frame count == parquet rows, duration vs last timestamp, fps matches `info.json`).
- `inspect`, `validate`, `--json`, exit codes, `--jobs N`.
- Local directories only.

Out (explicitly, until v0.1 ships): frame decoding, repair/fix, conversion, HDF5/RLDS/MCAP,
Hugging Face Hub access, Python bindings, TUI.

## Milestones (each ends with something runnable)
| # | Deliverable | Done when |
|---|---|---|
| M0 | Current scaffold (v2.x, metadata+parquet) | 9 tests green. **Done.** |
| M1 | Validate against **real** datasets | Run on 5 real Hub datasets (v2.1); zero false positives, or each triaged. Fixtures adjusted to real schemas. |
| M2 | Video container checks (`mp4` crate, moov only) | Test fixtures with real tiny mp4s: frame-count mismatch and duration drift caught; <50 ms per file. |
| M3 | v3.0 support | Same checks pass on 3 real v3 datasets (episodes packed per file, metadata in parquet). |
| M4 | Parallelism + `--json` + benchmark | rayon over episodes; published table vs lerobot-doctor/physlint on the same dataset (fair, reproducible script). |
| M5 | Release v0.1 | README with real output, crates.io publish, GitHub release binaries (mac/linux), GitHub Action example. Post in LeRobot Discord / HF forum. |

Later (only if v0.1 gets pulled by users): opt-in frame decode for frozen/black frames, HF Hub
range-request validation (no full download), `--fix` for safe repairs, PyO3 wrapper.

## Success signal (decide before building more)
After M5, within 6 weeks: >=3 people outside your circle file an issue or run it in CI, or it gets
mentioned in someone else's repo. If none: stop at v0.1, do not expand scope.

## Risks
- Real datasets differ from my fixtures (M1 exists for this). Schema details were not verified
  against the LeRobot source, only inferred.
- v3 field names unverified; read LeRobot source/docs at M3 before coding.
- mp4 `moov` parsing: variable frame rate and B-frame edit lists can make "frame count" subtle.
  Treat as warnings first, errors once validated on real data.
- The market is small. This is a portfolio/credibility project more than a product.
