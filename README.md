# lecop

The police of your robot dataset. A fast, single-binary health check for
[LeRobot](https://github.com/huggingface/lerobot)-format datasets, written in Rust.

> **Unofficial.** Not affiliated with Hugging Face or the LeRobot project.
> **Early stage.** Tested only on synthetic fixtures so far, not yet on real Hub datasets.

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

`validate` currently checks (LeRobot **v2.x** layout):

- episode and frame counts in `info.json` vs `episodes.jsonl`, and episode index continuity
- every data parquet and video file exists
- parquet row count matches `episodes.jsonl`
- timestamps are monotonic and match `frame_index / fps` (1e-4 s tolerance)
- `frame_index` runs `0..n-1`
- no NaN/Inf in float features
- warns if `action` is identical on every frame

Each finding is one line (format shown for illustration):

```
ERROR   ep 3      timestamp_drift          frame 10: t=1.05000s, expected 1.00000s at 10 fps
```

## Not yet supported

- LeRobot **v3.0** (rejected with a clear error)
- Video checks beyond file existence (container-level frame count and duration checks are next)
- JSON output, parallelism, remote (Hub) datasets

See [PLAN.md](PLAN.md) for the roadmap.

## Development

```bash
cargo test
```

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
