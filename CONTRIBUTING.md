# Contributing

Thanks for helping. lecop is a fast, single-binary health checker for LeRobot datasets; speed and zero setup
matter more than the number of checks.

## Set up

```bash
cargo build
cargo test
```

Before opening a PR, all of these must pass (CI runs them too):

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

If you touch a hot path, run `cargo bench` and put the before and after numbers in the PR description
(`cargo bench -- --save-baseline before`, change, `cargo bench -- --baseline before`).

## Finding work

Open issues carry one priority label (`P0` highest to `P3`) and one type label. Pick the highest priority you
can. If you find a new piece of work, open an issue first with a `Priority: Pn, because ...` line.

## Making a change

1. Branch from `main`.
2. For a bug: write a failing test first, confirm it fails for the right reason, then fix it.
3. Add tests with the change. Synthetic datasets built in a temp dir are preferred over real ones.
4. Update `README.md` if you add a command, option, check, issue code or supported version.
5. Open a PR describing what failed or changed and why, with the evidence. Decisions, plans and measurements
   that explain the change go in a comment on the issue, not in a file in the repo.

Style is in [STYLE_GUIDE.md](STYLE_GUIDE.md).

## Real datasets

Real data lives in `datasets/` (git-ignored, never commit it). `scripts/fetch-datasets.sh` lists what is
missing and `--yes` downloads it; `scripts/datasets.tsv` pins each dataset and the counts lecop should report.
When a check changes what a dataset reports, update the counts in the same commit. A new check should be
confirmed on real data and with a mutation of a copy (for example change `fps` in `info.json` and expect
`timestamp_drift`), not only on synthetic fixtures.

## Security

Never commit tokens, `.env` files or credentials; CI scans for secrets.

## License

By contributing you agree your work is licensed under the project's MIT OR Apache-2.0 terms.
