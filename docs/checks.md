# Checks and features

## Features

- Reads LeRobot **v2.x** (one file per episode) and **v3.x** (episodes packed into shared files) datasets.
- Checks datasets in parallel; the report is identical for any `-j`.
- Prints `text`, `full`, `json` or GitHub annotation output, and exits 0, 1 or 2 so CI can act on it, see [Usage](usage.md).
- Ignores, selects or re-grades issue codes, per dataset if needed, and sets numeric limits, in [`lecop.toml`](usage.md#config-file).
- Repairs the totals in `meta/info.json` with `--fix`, and shows them first with `--preview`, see [Repairing](usage.md#repairing).
- Decodes video frames on request (`--decode`, opt-in build), see [Decoding frames](usage.md#decoding-frames).

## Checks

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
| The statistics match the data | Covered: count, min, max, mean and std of the `action` and `observation.*` float features, in `stats.json` (v3.x, v2.0) and per episode in `episodes_stats.jsonl` (v2.1), with tolerances above float32 rounding (warning) | `stats_mismatch` |

Beyond what LeRobot checks:

| What leCop checks | Severity | Issue code |
|---|---|---|
| NaN or Inf in float features | error | `non_finite` |
| `action` never changes | warning | `action_constant` |
| More than half the frames repeat the previous action (says where the robot first and last moves) | warning | `idle_episode` |
| Action identical for 3 s or more between its first and last change (`frozen_seconds` under `[numeric]`; a robot that rests that long also counts, rest at the start and end of the episode does not) | warning | `action_frozen` |
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

## Not yet supported

- Audio/video drift inside a video (frames are only decoded with `--decode`, see above)
- Remote (Hub) datasets

The roadmap is the [open issues](https://github.com/michele-xilo/lecop/issues), by priority.
