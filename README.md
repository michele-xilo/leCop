<p align="center">
  <img src="assets/logo.svg" alt="leCop: a robot cop with sunglasses" width="160">
</p>

<h1 align="center">leCop</h1>

<p align="center"><em>The police of your robot dataset.</em></p>

<p align="center">
  <a href="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml"><img src="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://app.codecov.io/gh/michele-xilo/leCop"><img src="https://codecov.io/gh/michele-xilo/leCop/graph/badge.svg" alt="codecov"></a>
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="license: MIT OR Apache-2.0">
  <img src="https://img.shields.io/badge/rust-stable-orange?logo=rust" alt="Rust">
</p>

A fast, single-binary health check for
[LeRobot](https://github.com/huggingface/lerobot)-format datasets, written in Rust.

> **Unofficial.** Not affiliated with Hugging Face or the LeRobot project.
> **Early stage.** Tested on synthetic fixtures and on 16 real Hub datasets (v2.1 and v3.0, official and community), one of them with real faults. No real v2.0 dataset is tested yet.

leCop reads a dataset's metadata and parquet files, and the container metadata of its videos, and reports the faults
that break or quietly degrade training: totals that disagree with the data, drifting timestamps, missing or truncated
files, videos with the wrong number of frames, constant or clipped signals, and more. It is one binary with nothing
else to install. It takes 8 to 30 ms on the datasets in the [speed table](#performance), 17x to 59x faster than
lerobot-doctor, because it never decodes video unless you ask for it with `--decode`.

## Install

```bash
git clone https://github.com/michele-xilo/lecop
cd lecop
cargo install --path .
```

## Usage

```bash
lecop inspect  path/to/dataset   # summary from metadata only
lecop .                          # same as `lecop validate .`: scan the current directory for datasets
lecop validate                   # scan the current directory for datasets
lecop validate path/to/dataset   # one dataset, or a directory that contains many
```

`validate` is the default command: `lecop <paths> [flags]` is `lecop validate <paths> [flags]`. `validate` takes any
number of paths. A path that holds `meta/info.json` is a dataset; any other directory is
searched for datasets (`.gitignore` is respected). Datasets are checked in parallel and reported in sorted order.

```
$ lecop validate datasets
datasets/c_so100_press_red_button: ERROR   ep 7      video_frame_count        observation.images.left has 591 frames, episode has 588
datasets/c_so101_my_task_50ep: WARNING ep 18     idle_episode             64% of frames repeat the previous action; ...
Found 22 errors and 6 warnings in 3 datasets.
```

A clean run prints `All checks passed!`. Exit code: 0 clean (warnings do not fail), 1 if any error, 2 if a dataset,
the config or an argument cannot be read. A dataset whose metadata cannot be loaded (missing or invalid `meta/info.json`,
unsupported version, `fps` not positive) is reported as `cannot_open`: none of its other checks run, and `--ignore`,
`--select` and the config cannot hide it.

| Option | |
|---|---|
| `--format text\|full\|json\|github` (alias `--output-format`) | `full` adds the location and a one-line explanation per finding; `json` is one object (`errors`, `warnings`, `ignored`, `issues[]`); `github` prints `::error`/`::warning` annotations |
| `--statistics` | findings per issue code |
| `-q`, `--quiet` | summary only, nothing on success |
| `--no-color` | colour is also off when piped or when `NO_COLOR` is set |
| `--ignore a,b`, `--select a,b` | drop, or keep only, these issue codes |
| `--decode` | decode the video frames and report undecodable, frozen and black cameras; needs a build with `--features decode`, see below |
| `--fix`, `--preview` | apply the safe repairs, or show them without changing anything (`--diff` is an alias), see below |
| `--config FILE`, `--isolated` | use this config, or none |
| `-v`, `-vv` | log progress, then per-file timings, to stderr (never stdout); `RUST_LOG` overrides |
| `-j N` | worker threads (default: one per core); output is identical for every `-j` |

### Repairing

`validate` alone never writes. `--fix` (also `lecop . --fix`) applies the only repairs leCop makes: the totals in
`meta/info.json` recomputed from metadata it does not touch, then validates again. `--preview` shows what `--fix` would
change and writes nothing. When a safe repair exists, a plain run ends with `[*] 1 dataset fixable with --fix` on stderr.

```bash
lecop validate --preview datasets/my_dataset   # prints what would change, on stderr
lecop validate --fix datasets/my_dataset    # applies it
```

| Finding | Repair |
|---|---|
| `frame_count` | `total_frames` becomes the sum of the episode lengths |
| `episode_count` | `total_episodes` becomes the number of episodes in the metadata |
| `tasks_count` | `total_tasks` becomes the number of tasks in the task metadata |

Nothing is repaired when the episode metadata and the data files disagree (`length_mismatch`, `episode_rows_missing`,
`data_missing`, `data_unreadable`, `timestamp_missing`, `episode_index`, `range_length`, `range_gap`, `index_mismatch`): a
total taken from that metadata could be as wrong as the one it replaces. Only the number changes, key order and
formatting stay. The original is kept as `meta/info.json.bak`, and `--fix` stops if that file already exists. Data
and video files are never edited.

### Decoding frames

The default build never decodes video: it reads container metadata only, which is why it takes milliseconds. To also
decode the frames, build with the `decode` feature and pass `--decode`:

```bash
cargo install --path . --features decode
lecop validate --decode datasets/my_dataset
```

H.264 is decoded with openh264 and AV1 with rav1d (no assembly, so it is the slow one). Per frame leCop keeps the mean
luma and the difference to the previous frame, so it reports frames the decoder rejects (`video_decode`), a camera that
repeats the same picture (`video_frozen`) and one that shows black (`video_black`). The output ends with
`Video frames were decoded (--decode).` when it ran. A video in another codec is reported as `not decoded`, not skipped.
Measured on an Apple M2 Pro, release build: 8 s for `c_so101_pill_sorting_medoc`, 12 s for `c_careattrs_mug2v2`, 64 s for
`c_so100_press_red_button` and 146 s for `c_so101_my_task_50ep` (AV1); 1 s for `c_d3il_avoiding_vision_224` (H.264). The
feature adds about 1.9 MB to the binary (10.9 MB against 9.0 MB).

### Config file

`lecop.toml` in the scanned directory or any parent (nearest wins):

```toml
ignore = ["idle_episode"]          # drop these codes
# select = ["frame_count"]         # or keep only these
# error = ["video_fps"]            # promote a warning to an error
# warn = ["frame_count"]           # demote an error to a warning

[per-dataset."datasets/messy"]     # path relative to this file
ignore = ["video_frame_count"]

[limits."action"]                  # only `action` and `observation.*` features
min = -1.0                         # a number, or one value per dimension: [-1.0, -0.5, ...]
max = [1.0, 1.0, 0.5]
max_step = 0.2                     # largest change of any action dimension between two frames

[training]
chunk_size = 100                   # frames a policy window spans; episodes shorter than this are reported

[numeric]
clipped_fraction = 0.05            # report a dimension with this fraction of its values (or more) on its min or max
```

Unknown issue codes are an error. Ignored findings are counted: `Ignored by config: idle_episode (5)`.

## How leCop compares with other tools

What [lerobot-doctor](https://pypi.org/project/lerobot-doctor/) 0.2.0 and
[physlint](https://pypi.org/project/physlint/) 0.4.0 check, read from their installed rule lists and source,
against what leCop checks. physlint also validates MCAP and ROS 2 recordings, which is out of scope here.
✅ checked, ⚠️ partly checked or needs thresholds you set, ❌ not checked.

| Check | leCop | lerobot-doctor | physlint |
|---|---|---|---|
| `info.json` readable, version supported, `fps` positive | ✅ | ✅ | ✅ |
| `info.json` required fields present | ✅ | ✅ | ✅ |
| `total_episodes`, `total_frames` match the metadata | ✅ | ✅ | ✅ |
| `total_tasks` matches the task metadata | ✅ | ✅ | ❌ |
| Data and episode-metadata files exist and are readable | ✅ | ✅ | ✅ |
| Episode ids unique, contiguous, boundaries and global `index` valid | ✅ | ⚠️ | ✅ |
| Timestamps monotonic, match `fps`, no dropped-frame gaps | ✅ | ✅ | ✅ gap limit by config |
| `frame_index` sequential within each episode | ✅ | ✅ | ❌ |
| Declared dtype and shape match the stored data | ✅ float list widths | ⚠️ | ✅ |
| Same features in every episode (missing, extra, dtype, shape) | ✅ | ✅ | ⚠️ |
| Required streams present (`observation.state`, ...) | ⚠️ `action`, `observation.state`, `timestamp` | ❌ | ⚠️ needs config |
| Streams cover whole episodes; observation/action delay | ❌ | ❌ | ✅ delay by config |
| NaN and Inf in feature columns | ✅ | ✅ | ✅ |
| Numeric limits per feature | ✅ by config | ❌ | ⚠️ needs config |
| Implausible jumps between frames | ✅ also `max_step` by config | ✅ | ⚠️ needs config |
| Constant or frozen action, idle episodes | ✅ | ✅ | ❌ |
| Stuck or constant dimensions, zero variance | ✅ | ✅ | ❌ |
| Values clipped at their min or max | ✅ 5% of frames or more, configurable | ✅ | ❌ |
| Extreme outliers, distribution shift across episodes | ✅ | ✅ | ❌ |
| Duplicate episodes | ✅ exact action sequences | ✅ near-duplicates | ✅ exact |
| Episode length: empty, too short, outliers | ✅ | ✅ | ❌ |
| Statistics file present and covering every feature | ✅ | ✅ | ❌ |
| Statistics match the data (count, min, max, mean, std) | ✅ `stats.json`, v3.x and v2.0 | ❌ compares feature names only | ❌ |
| Video file exists, readable mp4, frame rate | ✅ | ✅ | ✅ |
| Video frame count against the episode (container tables) | ✅ | ✅ | ❌ |
| Video resolution against `info.json` | ✅ | ✅ | ❌ |
| Frames decode, frozen frames, black frames | ✅ opt-in `--decode` (H.264 and AV1) | ⚠️ first frame only | ✅ |
| Portability: absolute paths, symlinks, stray files in `data/`, oversized files | ✅ | ✅ | ❌ |
| Training readiness: zero-std statistics, episodes shorter than the policy chunk | ✅ | ✅ | ❌ |
| Only leCop: v3 video span checks, `range_*`, `index_mismatch` | ✅ | ❌ | ❌ |

Gaps are tracked as issues by priority; see the open issues labelled `feature`.

## Performance

How long each tool takes to check the same dataset: median of 3 runs after one warm-up, warm page cache, wall time
including process start. Apple M2 Pro, Python 3.12, leCop `release` build, lerobot-doctor 0.2.0 and physlint 0.4.0
installed from PyPI. physlint reads v3 only. Reproduce with `scripts/compare.py`.

| Dataset | leCop | lerobot-doctor | physlint | leCop speedup |
|---|---|---|---|---|
| `pusht` (v2.1) | 🏆 **15 ms** | 745 ms | n/a | 50x |
| `svla_so101_pickplace` (v2.1) | 🏆 **9 ms** | 483 ms | n/a | 54x |
| `c_careattrs_mug2v2` (v2.1) | 🏆 **8 ms** | 464 ms | n/a | 58x |
| `v3_pusht` (v3.0) | 🏆 **11 ms** | 646 ms | 16,269 ms | 59x |
| `v3_aloha_sim_insertion_human` (v3.0) | 🏆 **10 ms** | 483 ms | 2,916 ms | 48x |
| `v3_aloha_sim_transfer_cube_human` (v3.0) | 🏆 **10 ms** | 465 ms | 3,486 ms | 46x |
| `c_so101_pill_sorting_medoc` (v3.0) | 🏆 **30 ms** | 506 ms | 741 ms | 17x |
| `c_so101_my_task_50ep` (v3.0) | 🏆 **20 ms** | 511 ms | 7,934 ms | 26x |
| `c_so101_block_box_50episodes_final` (v3.0) | 🏆 **13 ms** | 419 ms | 2,839 ms | 32x |
| `c_black-stressball-tutorial` (v3.0) | 🏆 **15 ms** | 493 ms | 7,900 ms | 33x |

The tools check different things (see the comparison above), so this compares speed only.

## Checks and features

### Features

- Reads LeRobot **v2.x** (one file per episode) and **v3.x** (episodes packed into shared files) datasets.
- Checks datasets in parallel; the report is identical for any `-j`.
- Prints `text`, `full`, `json` or GitHub annotation output, and exits 0, 1 or 2 so CI can act on it, see [Usage](#usage).
- Ignores, selects or re-grades issue codes, per dataset if needed, and sets numeric limits, in [`lecop.toml`](#config-file).
- Repairs the totals in `meta/info.json` with `--fix`, and shows them first with `--preview`, see [Repairing](#repairing).
- Decodes video frames on request (`--decode`, opt-in build), see [Decoding frames](#decoding-frames).

### Checks

LeRobot does not publish a list of dataset checks. The table is derived from its
[v3 format docs](https://github.com/huggingface/lerobot/blob/main/docs/source/lerobot-dataset-v3.mdx) and from what its
[loader and writer](https://github.com/huggingface/lerobot/tree/main/src/lerobot/datasets) enforce or assume.
All checks read metadata and parquet only: no video frame is decoded.

| What LeRobot expects | Status | Issue code |
|---|---|---|
| `meta/info.json` exists, parses, has a supported `codebase_version` | Covered (v2.x, v3.x; other versions are rejected; v2.0 is read like v2.1, with `stats.json`) | `cannot_open`; a missing required field is named in the message |
| `fps` is a positive number | Covered | `cannot_open` |
| `total_episodes` matches the episode metadata | Covered | `episode_count` |
| `total_frames` matches the sum of episode lengths | Covered | `frame_count` |
| Episode indices run `0..n-1` | Covered | `episode_index` |
| Episode frame ranges match their length, are contiguous, and the global `index` column agrees (v3) | Covered | `range_length`, `range_gap`, `index_mismatch` |
| Every data parquet exists and is readable (an unfinalized or truncated file is not) | Covered | `data_missing`, `data_unreadable` |
| Each episode has the rows its metadata says | Covered | `length_mismatch`, `episode_rows_missing` |
| `timestamp` equals `frame_index / fps` within 1e-4 s, and increases | Covered | `timestamp_drift`, `timestamp_not_monotonic` |
| `frame_index` runs `0..n-1` within each episode | Covered | `frame_index` |
| The `timestamp` column exists and is a float | Covered | `timestamp_missing` |
| Every declared feature has a data column of the declared dtype and width; no undeclared columns | Covered (list width for float features only; multi-dimensional shapes are not checked) | `feature_missing`, `feature_dtype`, `feature_shape`, `column_shape`, `feature_extra` (warning) |
| Every video file exists and is a readable mp4 | Covered | `video_missing`, `video_unreadable` |
| Video frame rate matches `fps` | Covered (warning) | `video_fps` |
| Video frame size matches the shape in `info.json` | Covered, from the container's sample description | `video_resolution` |
| Video holds the episode's frames: v2 frame count, v3 span length, bounds and frames in the span | Covered, from the container's sample tables | `video_frame_count`, `video_span_length`, `video_span_beyond_end` |
| An `action` feature exists (policies expect that name) | Covered (warning; `actions` is accepted for the other checks) | `nonstandard_features` |
| `observation.state` exists | Covered (warning) | `nonstandard_features` |
| Camera features are video-backed, not images | Not yet | |
| `total_tasks` matches the task metadata (`tasks.jsonl` lines, `tasks.parquet` rows) | Covered; the file missing is an error, a different count a warning | `tasks_missing`, `tasks_count` |
| Every `task_index` in the data is in the task metadata | Covered | `task_index` |
| The statistics file (`stats.json`, or `episodes_stats.jsonl` for v2.1) exists and has an entry for every non-string feature | Covered (warnings; v2.1 reads the first episode's entry only) | `stats_missing`, `stats_feature_missing` |
| The statistics match the data | Covered for `stats.json` (v3.x, v2.0): count, min, max, mean and std of the `action` and `observation.*` float features, with tolerances far above float32 rounding (warning). v2.1 per-episode statistics: not yet | `stats_mismatch` |

Beyond what LeRobot checks:

| What leCop checks | Severity | Issue code |
|---|---|---|
| NaN or Inf in float features | error | `non_finite` |
| `action` never changes | warning | `action_constant` |
| More than half the frames repeat the previous action (says where the robot first and last moves) | warning | `idle_episode` |
| Empty episode | error | `episode_empty` |
| Episode shorter than a second | warning | `episode_short` |
| Episode length more than 3 standard deviations from the mean (judged from 10 episodes up) | warning | `episode_length_outlier` |
| Episode whose action sequence repeats an earlier one | warning | `duplicate_episode` |
| Dimension of `action` or `observation.*` that never changes | warning | `constant_dimension` |
| Dimension with 5% or more of its frames on its minimum or maximum (`clipped_fraction` under `[numeric]`; a gripper that stays closed counts) | warning | `clipped_dimension` |
| Values more than 10 standard deviations from the mean of their dimension (judged from 100 frames up) | warning | `outlier_values` |
| A frame-to-frame action change whose mean over the dimensions is more than 8 standard deviations from the usual step (up to 5 episodes listed, the rest counted) | warning | `action_jump` |
| The mean of the action or a state column in the last quarter of the episodes more than 3 standard deviations (of the first quarter) from the first quarter's (judged from 10 episodes up) | warning | `distribution_shift` |
| `data_path` or `video_path` is an absolute path (the dataset only works on this machine) | warning | `path_absolute` |
| A file in `data/` that is not parquet (`.DS_Store`, leftovers) | warning | `data_stray_file` |
| A symbolic link in the dataset (not reported inside a Hub cache `snapshots/` directory, where links are the layout) | warning | `symlink` |
| A file over 5 GB, the size the Hub recommends staying under | warning | `file_oversized` |
| With `--decode`: video frames the decoder rejects | error | `video_decode` |
| With `--decode`: 30 or more frames in a row identical to the one before, or black | warning | `video_frozen`, `video_black` |
| A value below `min` or above `max` set under `[limits."feature"]` in `lecop.toml` (a list of the wrong width is `limit_width`) | error | `limit_out_of_range`, `limit_width` |
| The action changes between two frames by more than `max_step` | error | `limit_step` |
| A dimension of `action` or `observation.*` with zero std in `stats.json` (normalising divides by zero; not checked on v2.1) | warning | `stats_zero_std` |
| With `chunk_size` set under `[training]`: episodes shorter than the chunk size | warning | `episodes_below_chunk` |

Malformed files are reported, not crashed on.

### Not yet supported

- Audio/video drift inside a video (frames are only decoded with `--decode`, see above)
- Remote (Hub) datasets

The roadmap is the [open issues](https://github.com/michele-xilo/lecop/issues), by priority.

## Contributing

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
