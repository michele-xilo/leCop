<p align="center">
  <img src="assets/logo.svg" alt="leCop: a robot cop with sunglasses" width="160">
</p>

<h1 align="center">leCop</h1>

<p align="center"><em>The police of your robot dataset.</em></p>

<p align="center">
  <a href="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml"><img src="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/michele-xilo/lecop/badges/coverage.json" alt="coverage"></a>
  <a href="https://app.codecov.io/gh/michele-xilo/leCop"><img src="https://codecov.io/gh/michele-xilo/leCop/graph/badge.svg" alt="codecov"></a>
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
| Every declared feature has a data column of the declared dtype and width; no undeclared columns | Covered (list width for float features only; multi-dimensional shapes are not checked) | `feature_missing`, `feature_dtype`, `feature_shape`, `column_shape`, `feature_extra` (warning) |
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
warning that says where the robot first and last moves), empty, shorter-than-a-second and statistically long
or short episodes (`episode_empty`, `episode_short`, `episode_length_outlier`: more than 3 standard deviations from the
mean, judged from 10 episodes up) and an episode whose action sequence repeats an earlier one (`duplicate_episode`).
These last four are warnings except `episode_empty`. Malformed files are reported, not crashed on.

## Speed

How long each tool takes to check the same dataset: median of 3 runs after one warm-up, warm page cache, wall time
including process start. Apple M2 Pro, Python 3.12, leCop `release` build, lerobot-doctor 0.2.0 and physlint 0.4.0
installed from PyPI. physlint reads v3 only. Reproduce with `scripts/compare.py`.

| Dataset | leCop | lerobot-doctor | physlint | leCop speedup |
|---|---|---|---|---|
| `pusht` (v2.1) | 🏆 **14 ms** | 853 ms | n/a | 61x |
| `svla_so101_pickplace` (v2.1) | 🏆 **9 ms** | 578 ms | n/a | 64x |
| `c_careattrs_mug2v2` (v2.1) | 🏆 **7 ms** | 559 ms | n/a | 80x |
| `v3_pusht` (v3.0) | 🏆 **9 ms** | 702 ms | 16,235 ms | 78x |
| `v3_aloha_sim_insertion_human` (v3.0) | 🏆 **8 ms** | 581 ms | 2,951 ms | 73x |
| `v3_aloha_sim_transfer_cube_human` (v3.0) | 🏆 **8 ms** | 548 ms | 3,482 ms | 68x |
| `c_so101_pill_sorting_medoc` (v3.0) | 🏆 **30 ms** | 594 ms | 746 ms | 20x |
| `c_so101_my_task_50ep` (v3.0) | 🏆 **15 ms** | 576 ms | 7,695 ms | 38x |
| `c_so101_block_box_50episodes_final` (v3.0) | 🏆 **12 ms** | 472 ms | 2,726 ms | 39x |
| `c_black-stressball-tutorial` (v3.0) | 🏆 **13 ms** | 559 ms | 7,868 ms | 43x |

The tools check different things (see the coverage table below), so this compares speed only.

## Coverage compared with other tools

What [lerobot-doctor](https://pypi.org/project/lerobot-doctor/) 0.2.0 and
[physlint](https://pypi.org/project/physlint/) 0.4.0 check, read from their installed rule lists and source,
against what leCop checks. physlint also validates MCAP and ROS 2 recordings, which is out of scope here.
✅ checked, ⚠️ partly checked or needs thresholds you set, ❌ not checked, 🏆 the tool that does it best (no mark when all three are equal or none checks it).

| Check | leCop | lerobot-doctor | physlint |
|---|---|---|---|
| `info.json` readable, version supported, `fps` positive | ✅ | ✅ | ✅ |
| `info.json` required fields present | ⚠️ parse error | 🏆 **✅** | 🏆 **✅** |
| `total_episodes`, `total_frames` match the metadata | ✅ | ✅ | ✅ |
| `total_tasks` matches the task metadata | ❌ | 🏆 **✅** | ❌ |
| Data and episode-metadata files exist and are readable | ✅ | ✅ | ✅ |
| Episode ids unique, contiguous, boundaries and global `index` valid | 🏆 **✅** | ⚠️ | 🏆 **✅** |
| Timestamps monotonic, match `fps`, no dropped-frame gaps | ✅ | ✅ | ✅ gap limit by config |
| `frame_index` sequential within each episode | 🏆 **✅** | 🏆 **✅** | ❌ |
| Declared dtype and shape match the stored data | 🏆 **✅ float list widths** | ⚠️ | 🏆 **✅** |
| Same features in every episode (missing, extra, dtype, shape) | 🏆 **✅** | 🏆 **✅** | ⚠️ |
| Required streams present (`observation.state`, ...) | 🏆 **⚠️ `action`, `timestamp`** | ❌ | ⚠️ needs config |
| Streams cover whole episodes; observation/action delay | ❌ | ❌ | 🏆 **✅ delay by config** |
| NaN and Inf in feature columns | ✅ | ✅ | ✅ |
| Numeric limits per feature | ❌ | ❌ | 🏆 **⚠️ needs config** |
| Implausible jumps between frames | ❌ | 🏆 **✅** | ⚠️ needs config |
| Constant or frozen action, idle episodes | 🏆 **✅** | 🏆 **✅** | ❌ |
| Stuck or constant dimensions, zero variance | ❌ | 🏆 **✅** | ❌ |
| Values clipped at their min or max | ❌ | 🏆 **✅** | ❌ |
| Extreme outliers, distribution shift across episodes | ❌ | 🏆 **✅** | ❌ |
| Duplicate episodes | ✅ exact action sequences | ✅ near-duplicates | ✅ exact |
| Episode length: empty, too short, outliers | 🏆 **✅** | 🏆 **✅** | ❌ |
| `stats.json` present and consistent with the data | ❌ | 🏆 **✅** | ❌ |
| Video file exists, readable mp4, frame rate | ✅ | ✅ | ✅ |
| Video frame count against the episode (container tables) | 🏆 **✅** | 🏆 **✅** | ❌ |
| Video resolution against `info.json` | ❌ | 🏆 **✅** | ❌ |
| Frames decode, frozen frames, black frames | ❌ opt-in decode planned | ⚠️ first frame only | 🏆 **✅** |
| Portability: absolute paths, symlinks, unreadable or oversized files | ❌ | 🏆 **✅** | ❌ |
| Training readiness: normalisation, policy windows, `delta_timestamps` | ❌ | 🏆 **✅** | ❌ |
| Only leCop: v3 video span checks, `range_*`, `index_mismatch` | 🏆 **✅** | ❌ | ❌ |

Gaps are tracked as issues by priority; see the open issues labelled `feature`.

## Not yet supported

- Decoding frames (frozen or black frames, A/V drift inside a video)
- Remote (Hub) datasets

The roadmap is the [open issues](https://github.com/michele-xilo/lecop/issues), by priority.

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
