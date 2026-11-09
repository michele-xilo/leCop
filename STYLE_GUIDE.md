# Style guide

Rust code follows `cargo fmt` and `cargo clippy --all-targets -- -D warnings`. The rest is about judgement.

## Code

- Small, pure functions for logic (path templating, row runs, one timestamp check) so they can be tested
  without disk or a dataset. Keep functions short.
- Library code returns `anyhow::Result` with `.with_context(...)`, or records an `Issue` the user will see.
  No `unwrap()` or `expect()` outside tests unless the invariant is provable and commented.
- Runtime code first, `#[cfg(test)]` modules at the end of the file.
- Doc comments on public items whose intent is not obvious, not on every function.
- Prefer borrowing arrow value slices over copying, and no per-row allocation in hot loops.

## Comments

- Explain why, never what. Good: a LeRobot format quirk, a measured performance decision with the benchmark
  name, a counter-intuitive choice. Delete comments that restate the code.
- Do not cite PRs or specs in comments.
- A deliberate simplification with a known limit gets a `// ponytail:` comment naming the limit and the upgrade path.
- No emojis in code or comments.

## Public contract

Issue codes (`timestamp_drift`, ...), the CLI output format and exit codes are a contract. Add new ones; do
not rename or repurpose existing ones without a version bump. Warnings are for "probably wrong", errors for
"violates the format".

## Tests

- Plain `#[test]` functions with names that state the behaviour, for example
  `v3_flags_timestamp_drift_in_second_episode_of_a_file`.
- One healthy synthetic dataset that must report nothing, and one case per fault that asserts the exact issue code.
- Compare floats with a tolerance. Tests must pass offline on a clean clone; real datasets are optional and skipped when absent.

## Performance

No optimization without a benchmark, and no speed claim without a measured number from a release build. See
`.claude/rules/performance.md` for the method.

## Docs and commits

- Keep `README.md` in step with commands, options, checks and issue codes.
- Commit subject: short, imperative, with the area first (`validate: ...`, `dataset: ...`, `docs: ...`);
  the body says why when it is not obvious.
