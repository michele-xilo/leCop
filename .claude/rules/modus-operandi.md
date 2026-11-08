# Modus Operandi

## What this project is

lecop is an unofficial, fast, single-binary health checker for robot-learning datasets in the
[LeRobot](https://github.com/huggingface/lerobot) format (v2.x and v3.x), written in Rust. It is a library
crate (`src/lib.rs`: `dataset` resolves a dataset's episodes to files, `validate` runs the checks) plus a thin
CLI (`src/main.rs`: `inspect`, `validate`). Its reason to exist is speed and zero setup, not check count: see
`PLAN.md` and `performance.md`. The roadmap is `PLAN.md`; the decision log is `docs/agent/decisions.md`.

## Build, lint, format, test: do this often

Run these frequently during development, not only at the end.

```bash
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
cargo bench        # when you touched a hot path, see performance.md
```

Run all the checks before considering work done. Tests verify the code, not real data: a change to what the
checks report must also go through a real run of `cargo run --release -- validate datasets/<name>` on the
datasets in `datasets/` (see below), plus a mutation test on a copy (e.g. change `fps` in `info.json` and
expect `timestamp_drift` errors), so you know the checks fire rather than silently skip. If you cannot run
something (no network, dataset not downloaded), say so rather than claim it works.

## Code style

- Comments explain **why**, never what or how. A useful comment captures something the reader can't recover
  from the code: a LeRobot format quirk, a measured perf decision, a counter-intuitive choice. Strip comments
  that paraphrase the code. Comments do not cite specs or PRs; point to a human doc in `docs/` if a pointer
  helps. Mark deliberate simplifications that have a known ceiling with a `ponytail:` comment naming the
  ceiling and the upgrade path.
- Doc comments on public items where the intent is not obvious; not on every function.
- No emojis in code or comments.
- Keep functions short and focused. Prefer small pure functions (path templating, row-run computation,
  timestamp checks) that can be tested without touching disk.
- Never let errors pass silently. In library code return `anyhow::Result` with context (`.with_context(...)`),
  or record an `Issue` the user will see. No `unwrap()` / `expect()` in non-test code unless the invariant is
  provably upheld and commented.
- Rust files: all runtime code first, then the `#[cfg(test)]` module(s) at the end, never
  code-test-code-test. Clippy's `items_after_test_module` (on with `-D warnings`) enforces that no item
  follows a test module.
- Issue codes (`timestamp_drift`, `range_length`, ...), the CLI's output format and exit codes are a public
  contract once released. Add new ones; do not rename or repurpose existing ones without a version bump.
- Never decode video frames in the default path: video checks read container metadata only (see `PLAN.md`).

## Testing

- Every public function should have tests. Put pure logic where it can be tested without disk, network or a
  real dataset.
- Smoke tests build tiny synthetic datasets in a temp dir (`tests/validate.rs`): one healthy case that must
  report zero issues, and one case per fault that asserts the exact issue code. Keep the fixture writers
  faithful to the real schemas (verified against LeRobot's source and real datasets, not memory).
- Real datasets live in `datasets/` (git-ignored, never committed). Tests that need them must skip with a
  clear message when absent; the default `cargo test` must pass on a clean clone with no network.
- Match the existing style in `tests/`: plain `#[test]` functions, descriptive names that state the
  behaviour (`v3_flags_timestamp_drift_in_second_episode_of_a_file`), `tempfile` for files.
- Compare floats with an explicit tolerance, never `==`.

## Results and reproducibility

- Every reported number (speed, memory, issue counts) names the dataset (or synthetic set and seed), the
  lecop commit, the Rust version, the machine and the thread count. Throughput claims come from a measured
  run (criterion or `hyperfine`), never from reasoning.
- Do not compare against other tools unless the script that produced the comparison is in the repo and fair
  (same dataset, same machine, warm cache stated).

## Specs, decisions, notes

- `docs/agent/` holds everything meant for AI agents rather than people: the decision log
  (`decisions.md`, append, do not rewrite), working notes, conventions. Record a decision there when it
  changes scope, a format assumption, a dependency, or a public contract.
- Verified-versus-assumed matters: when a note rests on an assumption about the LeRobot format, say so, and
  update it once real data confirms or refutes it.
- Everything else in `docs/` and the top-level `README.md` is for people who use the project: how to run it,
  what it checks, what it does not support.

## Data and secrets

- Never commit datasets, `datasets/`, `target/`, Hugging Face tokens, `.env` files or credentials.
- Downloading a dataset needs the user's OK first: state the name, source and size.

## README

Keep `README.md` up to date in the same change when you add a command, option, check, issue code or
supported format version. A README that claims less or more than the code does is a bug.

## Git

- Branch from `main` for feature work. The repository is public: ask before pushing or publishing
  (crates.io, releases).
- Commit messages: short imperative summary, prefixed with the area where it helps
  (`validate: ...`, `dataset: ...`, `docs: ...`), body explains *why* if non-obvious.
