<p align="center">
  <img src="assets/logo.svg" alt="lecop" width="96">
</p>

<h1 align="center">lecop</h1>

<p align="center"><em>The police of your robot dataset.</em></p>

[![CI](https://github.com/michele-xilo/lecop/actions/workflows/ci.yml/badge.svg)](https://github.com/michele-xilo/lecop/actions/workflows/ci.yml)

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

- episode and frame counts in `info.json` vs the episode metadata (`episodes.jsonl` in v2, `meta/episodes/` in v3), and episode index continuity
- every data parquet and video file exists
- **video, without decoding a frame**: each mp4's frame count and duration are read from its container metadata. v2: frame count equals the episode length. v3: the episode's `[from, to)` span inside the shared video matches its length, lies within the video, and holds exactly as many frames as the episode has (dropped or duplicated frames show up as `video_frame_count`). A video whose frame rate differs from `info.json` is a warning, and a truncated or corrupt file is reported as unreadable
- parquet row count matches `episodes.jsonl`
- timestamps are monotonic and match `frame_index / fps` (1e-4 s tolerance)
- `frame_index` runs `0..n-1`
- no NaN/Inf in float features
- warns if `action` is identical on every frame, or if more than half of an episode's frames repeat the previous action (`idle_episode`: dead time, with the frames where it first and last changes)
- warns if there is no `action` feature (policies expect that name); action checks then use `actions` if present
- malformed data is reported, not crashed on: a missing or non-float `timestamp` column, ragged or wrongly shaped feature columns, `fps` that is not a positive number
- v3 only: each episode's frame range matches its length, ranges are contiguous, and the global `index` column agrees

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
