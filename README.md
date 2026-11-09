<p align="center">
  <img src="assets/logo.svg" alt="lecop" width="96">
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
lecop validate path/to/dataset   # exits 1 if any error is found
lecop validate path/to/dataset -j 4   # worker threads (default: one per core)
```

The report and exit code are identical for every `-j`.

`--format json` prints one object (`errors`, `warnings`, `issues[]` with `severity`, `episode`, `code`, `message`) for
scripts; `--format github` prints `::error`/`::warning` annotations for GitHub Actions. The exit code is 1 on
any error in every format.

`validate` supports LeRobot **v2.x** (one file per episode) and **v3.x** (episodes packed into shared files). It checks:

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

Each finding is one line (format shown for illustration):

```
ERROR   ep 3      timestamp_drift          frame 10: t=1.05000s, expected 1.00000s at 10 fps
```

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
