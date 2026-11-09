<p align="center">
  <img src="assets/logo.svg" alt="lecop: a robot cop with sunglasses" width="160">
</p>

<h1 align="center">lecop</h1>

<p align="center"><em>The police of your robot dataset.</em></p>

<p align="center">
  <a href="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml"><img src="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/michele-xilo/lecop/badges/coverage.json" alt="coverage"></a>
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="license: MIT OR Apache-2.0">
  <img src="https://img.shields.io/badge/rust-stable-orange?logo=rust" alt="Rust">
</p>

A fast, single-binary health check for
[LeRobot](https://github.com/huggingface/lerobot)-format datasets, written in Rust.

> **Unofficial.** Not affiliated with Hugging Face or the LeRobot project.
> **Early stage.** Tested on synthetic fixtures and on a handful of small official Hub datasets (v2.1 and v3.0), all clean. Not yet tried on messy community datasets.

## Install

```bash
git clone https://github.com/michele-xilo/lecop
cd lecop
cargo install --path .
```

## Usage

```bash
lecop inspect  path/to/dataset   # summary from metadata only
lecop validate                   # scan the current directory for datasets
lecop validate path/to/dataset   # one dataset, or a directory that contains many
```

`validate` takes any number of paths. A path that holds `meta/info.json` is a dataset; any other directory is
searched for datasets (`.gitignore` is respected). Datasets are checked in parallel and reported in sorted order.

```
$ lecop validate datasets
datasets/c_so100_press_red_button: ERROR   ep 7      video_frame_count        observation.images.left has 591 frames, episode has 588
datasets/c_so101_my_task_50ep: WARNING ep 18     idle_episode             64% of frames repeat the previous action; ...
Found 22 errors and 6 warnings in 3 datasets.
```

A clean run prints `All checks passed!`. Exit code: 0 clean (warnings do not fail), 1 if any error, 2 if a dataset,
the config or an argument cannot be read.

| Option | |
|---|---|
| `--format text\|full\|json\|github` (alias `--output-format`) | `full` adds the location and a one-line explanation per finding; `json` is one object (`errors`, `warnings`, `ignored`, `issues[]`); `github` prints `::error`/`::warning` annotations |
| `--statistics` | findings per issue code |
| `-q`, `--quiet` | summary only, nothing on success |
| `--no-color` | colour is also off when piped or when `NO_COLOR` is set |
| `--ignore a,b`, `--select a,b` | drop, or keep only, these issue codes |
| `--config FILE`, `--isolated` | use this config, or none |
| `-v`, `-vv` | log progress, then per-file timings, to stderr (never stdout); `RUST_LOG` overrides |
| `-j N` | worker threads (default: one per core); output is identical for every `-j` |

### Config file

`lecop.toml` in the scanned directory or any parent (nearest wins):

```toml
ignore = ["idle_episode"]          # drop these codes
# select = ["frame_count"]         # or keep only these
# error = ["video_fps"]            # promote a warning to an error
# warn = ["frame_count"]           # demote an error to a warning

[per-dataset."datasets/messy"]     # path relative to this file
ignore = ["video_frame_count"]
```

Unknown issue codes are an error. Ignored findings are counted: `Ignored by config: idle_episode (5)`.

`validate` supports LeRobot **v2.x** (one file per episode) and **v3.x** (episodes packed into shared files).

LeRobot does not publish a list of dataset checks. The table is derived from its
[v3 format docs](https://github.com/huggingface/lerobot/blob/main/docs/source/lerobot-dataset-v3.mdx) and from what its
[loader and writer](https://github.com/huggingface/lerobot/tree/main/src/lerobot/datasets) enforce or assume.
All checks read metadata and parquet only: no video frame is decoded.

| What LeRobot expects | Status | Issue code |
|---|---|---|
| `meta/info.json` exists, parses, has a supported `codebase_version` | Covered (v2.x, v3.x; other versions are rejected, v2.0 is untested) | error on open |
| `fps` is a positive number | Covered | error on open |
| `total_episodes` matches the episode metadata | Covered | `episode_count` |
| `total_frames` matches the sum of episode lengths | Covered | `frame_count` |
| Episode indices run `0..n-1` | Covered | `episode_index` |
| Episode frame ranges match their length, are contiguous, and the global `index` column agrees (v3) | Covered | `range_length`, `range_gap`, `index_mismatch` |
| Every data parquet exists and is readable (an unfinalized or truncated file is not) | Covered | `data_missing`, `data_unreadable` |
| Each episode has the rows its metadata says | Covered | `length_mismatch`, `episode_rows_missing` |
| `timestamp` equals `frame_index / fps` within 1e-4 s, and increases | Covered | `timestamp_drift`, `timestamp_not_monotonic` |
| `frame_index` runs `0..n-1` within each episode | Covered | `frame_index` |
| The `timestamp` column exists and is a float | Covered | `column_missing` |
| Feature columns match the dtype and shape in `info.json` | Partial: float list columns are checked for consistent width, declared dtype and shape are not | `column_shape` |
| Every video file exists and is a readable mp4 | Covered | `video_missing`, `video_unreadable` |
| Video frame rate matches `fps` | Covered (warning) | `video_fps` |
| Video holds the episode's frames: v2 frame count, v3 span length, bounds and frames in the span | Covered, from the container's sample tables | `video_frame_count`, `video_span_length`, `video_span_beyond_end` |
| An `action` feature exists (policies expect that name) | Covered (warning; `actions` is accepted for the other checks) | `nonstandard_features` |
| `observation.state` exists | Not yet | |
| Camera features are video-backed, not images | Not yet | |
| `meta/tasks` exists and every `task_index` is in it | Not yet | |
| `meta/stats.json` exists and matches the data | Not yet | |

Beyond what LeRobot checks: NaN or Inf in float features (`non_finite`), an `action` that never changes
(`action_constant`), and episodes where more than half the frames repeat the previous action (`idle_episode`, a
warning that says where the robot first and last moves). Malformed files are reported, not crashed on.

## Speed

Library time of `validate` on a warm page cache (criterion, `cargo bench --bench validate`), release profile.
Machine: Apple M2 Pro (6 performance + 4 efficiency cores), rustc 1.99.1, commit `a85d39d` plus the read-path change below.
Synthetic datasets: 1,000 episodes x 600 frames, 14-dim action and state, one AV1-style mp4 per camera, fixed seed, healthy.

| Dataset | Before (sequential) | 1 thread | 10 threads |
|---|---|---|---|
| synthetic v2, 600k frames, 1000 files | 174.8 ms | 118.5 ms | 36.2 ms |
| synthetic v3, 600k frames, 10 data files | 91.3 ms | 95.7 ms | 21.1 ms |
| real `v3_aloha_sim_insertion_human` (87 MB, 50 ep) | 3.6 ms | n/a | 1.5 ms |
| real `svla_so101_pickplace` (84 MB, 50 ep) | 6.9 ms | n/a | 3.0 ms |
| real `c_careattrs_mug2v2` (84 MB, 30 ep) | 4.5 ms | n/a | 2.1 ms |

What moved the numbers: files and videos are checked in parallel (rayon), and each data file is read with one
`read` call instead of parquet's many small preads, which had stopped scaling past ~3 threads (v2, 10 threads:
98 ms before the change, 42 ms after). v3 is limited by its 10 files: one file is decoded by one thread.
The CLI wall time on the real datasets (6-8 ms for the three above, 27 ms for `c_so101_pill_sorting_medoc`) includes process start.

### Coverage compared with other checkers

What [lerobot-doctor](https://pypi.org/project/lerobot-doctor/) 0.2.0 and
[physlint](https://pypi.org/project/physlint/) 0.4.0 check, read from their installed rule lists and source,
against what lecop checks. physlint also validates MCAP and ROS 2 recordings, which is out of scope here.
`yes` means the tool reports it, `config` means it needs thresholds you set, `no` means it does not check it.

| Check | lecop | lerobot-doctor | physlint |
|---|---|---|---|
| `info.json` readable, version supported, `fps` positive | yes | yes | yes |
| `info.json` required fields present | partial (parse error) | yes | yes |
| `total_episodes`, `total_frames` match the metadata | yes | yes | yes |
| `total_tasks` matches the task metadata | no | yes | no |
| Data and episode-metadata files exist and are readable | yes | yes | yes |
| Episode ids unique, contiguous, boundaries and global `index` valid | yes | partial | yes |
| Timestamps monotonic, match `fps`, no dropped-frame gaps | yes | yes | yes (gap limit by config) |
| `frame_index` sequential within each episode | yes | yes | no |
| Declared dtype and shape match the stored data | partial (width only) | partial | yes |
| Same features in every episode (missing, extra, dtype, shape) | no | yes | partial |
| Required streams present (`observation.state`, ...) | partial (`action`, `timestamp`) | no | config |
| Streams cover whole episodes; observation/action delay | no | no | yes (delay by config) |
| NaN and Inf in feature columns | yes | yes | yes |
| Numeric limits per feature | no | no | config |
| Implausible jumps between frames | no | yes | config |
| Constant or frozen action, idle episodes | yes | yes | no |
| Stuck or constant dimensions, zero variance | no | yes | no |
| Values clipped at their min or max | no | yes | no |
| Extreme outliers, distribution shift across episodes | no | yes | no |
| Duplicate episodes | no | yes (near-duplicates) | yes (exact) |
| Episode length: empty, too short, outliers | partial (rows missing) | yes | no |
| `stats.json` present and consistent with the data | no | yes | no |
| Video file exists, readable mp4, frame rate | yes | yes | yes |
| Video frame count against the episode (container tables) | yes | yes | no |
| Video resolution against `info.json` | no | yes | no |
| Frames decode, frozen frames, black frames | no (opt-in decode planned) | first frame only | yes |
| Portability: absolute paths, symlinks, unreadable or oversized files | no | yes | no |
| Training readiness: normalisation, policy windows, `delta_timestamps` | no | yes | no |
| Only lecop: v3 video span checks, `range_*`, `index_mismatch` | yes | no | no |

Gaps are tracked as issues by priority; see the open issues labelled `feature`.

### Speed compared with other checkers

`scripts/compare.py` times lecop against [lerobot-doctor](https://pypi.org/project/lerobot-doctor/) 0.2.0 and
[physlint](https://pypi.org/project/physlint/) 0.4.0 (installed from PyPI into separate venvs) on the same
datasets: median of 3 runs after one warm-up, warm page cache, wall time including process start, Apple M2 Pro,
Python 3.12, lecop `release` build. The tools check different things, so this is speed only, not coverage.
physlint reads v3 only.

| Dataset | lecop | lerobot-doctor | physlint |
|---|---|---|---|
| `pusht` (v2.1) | 14 ms | 853 ms | n/a |
| `svla_so101_pickplace` (v2.1) | 9 ms | 578 ms | n/a |
| `c_careattrs_mug2v2` (v2.1) | 7 ms | 559 ms | n/a |
| `v3_pusht` (v3.0) | 9 ms | 702 ms | 16,235 ms |
| `v3_aloha_sim_insertion_human` (v3.0) | 8 ms | 581 ms | 2,951 ms |
| `v3_aloha_sim_transfer_cube_human` (v3.0) | 8 ms | 548 ms | 3,482 ms |
| `c_so101_pill_sorting_medoc` (v3.0) | 30 ms | 594 ms | 746 ms |
| `c_so101_my_task_50ep` (v3.0) | 15 ms | 576 ms | 7,695 ms |
| `c_so101_block_box_50episodes_final` (v3.0) | 12 ms | 472 ms | 2,726 ms |
| `c_black-stressball-tutorial` (v3.0) | 13 ms | 559 ms | 7,868 ms |

## Not yet supported

- Decoding frames (frozen or black frames, A/V drift inside a video)
- Remote (Hub) datasets

See [PLAN.md](PLAN.md) for the roadmap.

## Development

```bash
cargo test
cargo bench   # criterion; see .claude/rules/performance.md

scripts/fetch-datasets.sh          # list real test datasets that are missing
scripts/fetch-datasets.sh --yes    # download them into datasets/ (git-ignored)
```

`cargo test` also checks the error and warning counts on every downloaded dataset listed in
`scripts/datasets.tsv` and skips the ones you do not have.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [STYLE_GUIDE.md](STYLE_GUIDE.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
