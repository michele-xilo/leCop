# Agent docs

Context for AI coding agents working on lecop. Not user documentation; humans should read the
top-level README.md and PLAN.md instead.

- `decisions.md`: decision log (what was decided, why, what would change it). Append, don't rewrite.
- `m1-notes.md`: working notes for M1 (testing on real datasets): datasets used, findings, status.

## Working conventions
- Build/test: `cargo test`. Smoke tests use synthetic fixtures in `tests/validate.rs`.
- Real datasets live in `datasets/` (gitignored, never commit). `scripts/datasets.tsv` pins repo, commit and
  expected counts; `scripts/fetch-datasets.sh --yes` downloads (ask the user first: state name and size).
- Bug fixes: write the failing test first (fixture reproducing the real-data case), then fix.
- Keep scope per PLAN.md. Ask before pushing; the repo is public.
