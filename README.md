# lecop

The police of your robot dataset. A fast, single-binary health check for
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
```

`validate` supports LeRobot **v2.x** (one file per episode) and **v3.x** (episodes packed into shared files). It checks:

- episode and frame counts in `info.json` vs the episode metadata (`episodes.jsonl` in v2, `meta/episodes/` in v3), and episode index continuity
- every data parquet and video file exists
- **video, without decoding a frame**: each mp4's frame count and duration are read from its container metadata. v2: frame count equals the episode length. v3: the episode's `[from, to)` span inside the shared video matches its length and lies within the video. A video whose frame rate differs from `info.json` is a warning, and a truncated or corrupt file is reported as unreadable
- parquet row count matches `episodes.jsonl`
- timestamps are monotonic and match `frame_index / fps` (1e-4 s tolerance)
- `frame_index` runs `0..n-1`
- no NaN/Inf in float features
- warns if `action` is identical on every frame, or if more than half of an episode's frames repeat the previous action (`idle_episode`: dead time, with the frames where it first and last changes)
- warns if there is no `action` feature (policies expect that name); action checks then use an `action*` feature such as `actions`
- malformed data is reported, not crashed on: a missing or non-float `timestamp` column, ragged or wrongly shaped feature columns, `fps` that is not a positive number
- v3 only: each episode's frame range matches its length, ranges are contiguous, and the global `index` column agrees

Each finding is one line (format shown for illustration):

```
ERROR   ep 3      timestamp_drift          frame 10: t=1.05000s, expected 1.00000s at 10 fps
```

## Not yet supported

- Decoding frames (frozen or black frames, A/V drift inside a video), and v3 per-episode frame counts inside a shared video
- JSON output, parallelism, remote (Hub) datasets

See [PLAN.md](PLAN.md) for the roadmap.

## Development

```bash
cargo test
cargo bench   # criterion; see .claude/rules/performance.md
```

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
