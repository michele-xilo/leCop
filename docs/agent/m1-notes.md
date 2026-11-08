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

## v3.0 results (2026-11-08)
- v3 support implemented (see decisions.md). Verified field names against LeRobot source
  (`utils.py`, `dataset_metadata.py`), not memory.
- Real v3 datasets in `datasets/v3_*`: pusht, aloha_sim_transfer_cube_human, aloha_sim_insertion_human.
  All report 0 errors / 0 warnings.
- Confirmed on real data: v3 data-file `timestamp` is episode-relative (matches frame_index/fps);
  pusht packs all 206 episodes into one data file and one episodes-metadata file.
- Mutation test on real v3 (fps 10 -> 11) produced 206 `timestamp_drift` errors, so checks run.
- Still untested on real data: files split across chunks, multi-file datasets, videos.

## Video checks (2026-11-08, branch feat/video-checks)
- All 8 real datasets (5 v2.1, 3 v3.0; AV1 video) report 0 errors / 0 warnings with video checks on;
  each validates in <=0.8 s wall time (release build, Apple M2 Pro, warm cache).
- Mutation tests on real copies: truncated v2 mp4 -> `video_unreadable` ("truncated mdat box ...");
  fps 10->12 in info.json -> `video_fps` warnings (v2: per episode; v3: once, the file is shared) and
  `video_span_length` errors for every v3 episode.
- Ground truth: pusht ep0 has 161 frames in episodes.jsonl and ffprobe; v3 pusht shared video has 25650
  frames in ffprobe, equal to total_frames.
- criterion `cargo bench --bench mp4` (Apple M2 Pro, rustc 1.99.1, release profile, synthetic files,
  moov last, warm cache, commit df4ecaa + uncommitted branch work):
  161 frames 12.7 us whether the mdat is 1 KiB or 64 MiB; 25,650 frames 18.1 us; 500,000 frames 81 us.
  Cost scales with the moov size, not with the video size.

## Code review findings fixed (2026-11-08)
Proven by failing tests first: mp4 box-size `pos + size` overflowed on a crafted 64-bit size; ragged
list columns panicked at a slice in `validate`; a missing `timestamp` column was misdiagnosed as a length
mismatch; `fps: 0` was accepted and silently disabled the time checks. Real-dataset results and benchmark
numbers unchanged after the fixes.

Known, not fixed: a v3 dataset whose episodes interleave across data/video files would re-read files
(only the latest file per data/camera is kept); a non-int64 `episode_index` column makes a v3 file look
like v2; no coverage tool installed (`cargo llvm-cov`); `validate` is still sequential.
