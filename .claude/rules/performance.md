# Performance

Speed is lecop's reason to exist (see `docs/agent/PLAN.md`: the Python validators already exist, we win on speed,
video checks without decoding, and zero setup). Treat it as a feature with the same rigour as correctness:
designed in, measured, and protected against regressions.

## Measure first, always

- No optimization without a benchmark that shows the problem, and no performance claim without a number
  from a measured run. Reasoning about "this should be faster" is not evidence.
- Benchmarks use [criterion](https://crates.io/crates/criterion) in `benches/` (`[[bench]]` with
  `harness = false`, `criterion` as a dev-dependency). Run with `cargo bench`.
- Every hot path has a benchmark. The hot paths are: loading episode metadata (v2 jsonl, v3 parquet),
  reading a data parquet file, the per-episode checks, the end-to-end `validate` over a whole dataset, and
  (when added) video container metadata parsing.
- Benchmark inputs are reproducible: synthetic datasets generated with a fixed seed and stated size (episodes,
  frames per episode, feature width), and, where relevant, the named real datasets in `datasets/`. Generate
  inputs outside the timed region.
- For the CLI end to end, use `hyperfine` on a named dataset with warm cache stated.
- Use `std::hint::black_box` on inputs and outputs so the optimizer cannot delete the work.

## Every performance change shows before and after

- Record a baseline before changing anything: `cargo bench -- --save-baseline before`, then after the
  change `cargo bench -- --baseline before`. Put the criterion comparison (or the hyperfine numbers) in the
  commit or PR description, with dataset, machine, Rust version and thread count.
- A change that regresses a benchmark by more than ~5% needs a stated reason and the user's OK.
- A performance regression found later is a bug: first add or extend the benchmark that exposes it, then
  fix it (the TDD loop applied to speed, see `TDD.md`).
- Find the bottleneck with a profiler (`cargo flamegraph`, `samply`, or `perf`) before optimizing. Fix the
  top item, re-measure, repeat. Do not optimize what the profile doesn't show.
- A measured, counter-intuitive performance decision gets a short "why" comment with the numbers or the
  benchmark name. An optimization that makes code harder to read and does not move a benchmark gets reverted
  (`general.md`: Simplicity First).

## Design rules for speed

1. **Parallelize across independent units.** Data files and episodes are independent: use
   [rayon](https://crates.io/crates/rayon) (`par_iter` over files; over episodes inside a file when a file
   holds many). Do not parallelize inside tiny loops; the unit of work must be large enough to amortize
   scheduling (rule of thumb: tens of microseconds or more).
2. **Deterministic output regardless of thread count.** Each task returns its own issues; merge them in a
   stable order (by episode index, then check order). The CLI output and exit code must be identical for
   `--jobs 1` and `--jobs N`. Test it (same dataset, both settings, equal results).
3. **No shared mutable state or locks in hot loops.** Prefer map/collect and reduce over `Mutex`. Avoid
   `Arc<Mutex<Vec<_>>>` for results.
4. **Bounded memory.** Peak memory should not scale with dataset size: at most one data file per worker in
   memory, released before the next. Use parquet row-group or batch streaming if a single file can be larger
   than a few hundred MB.
5. **Read only what you need.** Use parquet column projection (the checks need `timestamp`, `frame_index`,
   `episode_index`, `index` and the float feature columns; never read image/video columns). Read metadata
   before data, and fail fast on cheap checks before the expensive ones.
6. **Avoid copies and allocations in hot loops.** Borrow arrow arrays' value slices instead of converting
   to a new `Vec<f64>` when the type already matches; pre-size vectors (`with_capacity`); no per-row
   allocation, no per-row `format!`, no per-row hashing when an index or a contiguous run will do.
7. **I/O and CPU are different problems.** On cold storage the limit is I/O, on warm cache it is CPU. State
   which one a benchmark measures. Do not read a file twice; check existence with a single metadata call.
8. **Video: container metadata only.** Parse the `moov` sample tables, never decode frames, in the default
   path. Decoding is an explicit opt-in feature, never on by default.
9. **Expose the knob.** The CLI has `--jobs N` (default: number of cores); the library takes a thread pool or
   count rather than assuming a global one, so embedding callers control it.

## Build profile

- `[profile.release]` sets `opt-level = 3`, `lto = true`, `codegen-units = 1`. Benchmarks and any speed
  claim use the release (bench) profile, never debug.
- Do not set `target-cpu=native` for distributed binaries: they must run on other machines. It is fine for a
  local, clearly labeled experiment.
- New dependencies are judged on compile time, binary size and speed, not only features. Prefer arrow's
  typed arrays and the `mp4` crate's metadata path over anything that pulls in ffmpeg or a native library.

## Correctness is not negotiable

- A faster implementation must produce the same results: keep the old behaviour's tests, and add a test that
  the parallel path equals the sequential one on a synthetic dataset.
- Unsafe code only with a benchmark showing a meaningful win, a `SAFETY:` comment, and the user's OK.
- Speed never justifies skipping a check silently. If a check is made optional for speed, it is opt-in or
  opt-out by an explicit flag and the output says which checks ran.
