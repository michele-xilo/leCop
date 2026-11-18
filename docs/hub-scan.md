# Hub scan: what leCop reports on 75 public datasets

leCop was run on 75 public LeRobot datasets from the Hugging Face Hub, and every finding was checked by hand against the
data. Findings that turned out to be leCop bugs are excluded from the counts below and tracked as issues.

| | |
|---|---|
| Datasets | 75 (51 v3.0, 19 v2.1, 5 v2.0), 208 GB, 144,211 episodes, 26.8 million frames |
| Selection | top 1000 LeRobot datasets by 30-day Hub downloads; not gated; at most 10 GB (v2.0: 20 GB); at most 4 per owner; mirrors removed |
| No findings | 7 |
| Warnings only | 52 |
| At least one error | 16 |
| Crashes | 0 |
| leCop | 0.3.1, commit `4413e82`, release build |
| Machine | AMD Ryzen 9 7900X (24 threads), Linux, Rust 1.99.0, warm cache |
| Wall time | median of 5 runs each: 10.6 s for all 75 together, the slowest single dataset 2.9 s |

## Counts by issue code

Datasets affected and number of findings. An error means the dataset breaks a rule of the format; a warning is a
heuristic worth a look, not a verdict (see [checks.md](checks.md)).

| Code | Severity | Datasets | Findings |
|---|---|---|---|
| `video_span_length` | error | 5 | 92 |
| `episode_count` | error | 3 | 3 |
| `frame_count` | error | 3 | 3 |
| `episode_index` | error | 2 | 169 |
| `range_gap` | error | 2 | 83 |
| `data_missing` | error | 2 | 5 |
| `feature_missing` | error | 2 | 4 |
| `feature_dtype` | error | 2 | 4 |
| `video_resolution` | error | 2 | 2 |
| `index_mismatch` | error | 1 | 20 |
| `cannot_open` | error | 1 | 1 |
| `clipped_dimension` | warning | 51 | 164 |
| `episode_length_outlier` | warning | 44 | 519 |
| `outlier_values` | warning | 25 | 135 |
| `action_jump` | warning | 21 | 82 |
| `constant_dimension` | warning | 14 | 19 |
| `stats_feature_missing` | warning | 9 | 9 |
| `idle_episode` | warning | 8 | 148 |
| `distribution_shift` | warning | 8 | 28 |
| `stats_zero_std` | warning | 8 | 13 |
| `action_constant` | warning | 7 | 793 |
| `action_frozen` | warning | 7 | 20 |
| `nonstandard_features` | warning | 5 | 9 |
| `episode_short` | warning | 4 | 9348 |
| `duplicate_episode` | warning | 4 | 2631 |
| `stats_mismatch` | warning | 3 | 3 |

The errors fall into a few kinds: `info.json` totals that were never updated (0 episodes) or that disagree after
episodes were appended, metadata pointing at data files that are not in the repository, columns whose type differs
from `info.json` or that are missing, video spans much longer than their episode, and declared video sizes or shapes
that disagree with the video.

A `--decode` run on 15 of the datasets adds 5 `video_frozen` warnings on one dataset and nothing else.

## Excluded as leCop bugs

- Multi-dimensional features stored as nested lists are reported as `feature_dtype` and `column_shape` errors
  (2 datasets, 3,449 findings): [#158](https://github.com/michele-xilo/leCop/issues/158).
- Video frames are counted by decode time, so videos with B-frames get false `video_frame_count` and
  `video_span_beyond_end` errors (1 dataset, 408 findings): [#159](https://github.com/michele-xilo/leCop/issues/159).

`clipped_dimension` also fires on binary gripper dimensions, which is noise rather than a fault:
[#160](https://github.com/michele-xilo/leCop/issues/160).

## Per-dataset results and reproducing

Each dataset's record, with its pinned revision and all findings, is in `site/data/runs/` and on the
[results site](https://michele-xilo.github.io/leCop/), except for the 3 datasets affected by #158 and #159: their
records are added once those bugs are fixed and the datasets are re-run. To repeat the scan:

```bash
scripts/fetch-scan-datasets.sh --yes      # the list in scripts/scan-datasets.tsv, 208 GB
cargo build --release
scripts/site_run.py --scan
scripts/site_run.py --scan --decode       # the 15 rows marked decode=yes
```
