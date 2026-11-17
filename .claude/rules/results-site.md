# Results site

The results site (GitHub Pages, built from `site/`) is the public list of what leCop reported on real datasets
and how fast it ran. It is only as good as its data, so every run on a real dataset feeds it.

## Every run on a real dataset adds a record

- Any run of leCop on a real dataset whose result we report or rely on (a benchmark, a comparison, the Hub scan,
  a re-measure after a release, a check that a fix did not move the real datasets) writes or updates that
  dataset's record in `site/data/runs/`, in the same PR as the work.
- Records come from `scripts/site_run.py`, never by hand: it collects the leCop version and commit, Rust version,
  machine, threads and date itself. Run it with a release build of the commit being reported.
- The dataset must be pinned in a registry file the runner reads (`scripts/datasets.tsv`, or the list the run
  belongs to such as `scripts/scan-datasets.tsv`), so every record names a repo and a revision. If the runner
  cannot read that list yet, extend the runner in the same PR rather than writing records another way.
- A re-run replaces the dataset's record (each record shows its leCop version). Do not keep stale records next
  to new ones.
- If a run measures something the schema cannot hold (`--decode`, peak memory, a skipped check), extend
  `site/data/schema.json` and the runner first, then add the record. Do not drop the measurement.
- `scripts/site_run.py --check` must pass. CI runs it.

## What a record may say

- Records name public datasets, as the site does by design. Findings are worded as "leCop reported", never as
  a verdict on the dataset.
- Check a finding by hand before it goes on the site under a dataset's name. A finding that turns out to be a
  false positive is a bug: open the issue, and regenerate the record once it is fixed. A confirmed fault gets a
  polite issue on the dataset's repository, before or when the record is published.
- Records are the one kind of findings file committed to the repo. Decisions, reviews and working notes about a
  run still go on the GitHub issue (`modus-operandi.md`).
