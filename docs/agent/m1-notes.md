# M1 notes: lecop on real datasets

Datasets (revision v2.1, in `datasets/`, gitignored): pusht, aloha_sim_transfer_cube_human,
aloha_sim_insertion_human, svla_so101_pickplace, xarm_lift_medium.

Findings (2026-11-08):
- `lecop validate` reports 0 errors / 0 warnings on all five. These are curated official datasets, so
  clean is expected; it is a no-false-positive result, not proof of coverage.
- Real parquet schemas match the fixtures: `action` / `observation.state` are
  `fixed_size_list<float>[N]`, `timestamp` is float32, `frame_index` int64.
- Checks verified live (not silently skipping): copying pusht and changing fps 10 -> 11 produced
  206 `timestamp_drift` errors.
- Not yet exercised on real data: nonfinite values, missing files, frozen action. Real *broken*
  datasets are needed for that; official ones are healthy.

Next: find genuinely messy community datasets (e.g. `jchandru08/so100_press_red_button`, v2.1, 1.3 GB)
to look for true positives. Then v3.0 support (see decisions.md).
