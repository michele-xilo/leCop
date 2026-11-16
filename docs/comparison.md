# How leCop compares with other tools

What [lerobot-doctor](https://pypi.org/project/lerobot-doctor/) 0.2.0 and
[physlint](https://pypi.org/project/physlint/) 0.4.0 check, read from their installed rule lists and source,
against what leCop checks. physlint also validates MCAP and ROS 2 recordings, which is out of scope here.
✅ checked, ⚠️ partly checked or needs thresholds you set, ❌ not checked.

| Check | leCop | lerobot-doctor | physlint |
|---|---|---|---|
| `info.json` readable, version supported, `fps` positive | ✅ | ✅ | ✅ |
| `info.json` required fields present | ✅ | ✅ | ✅ |
| `total_episodes`, `total_frames` match the metadata | ✅ | ✅ | ✅ |
| `total_tasks` matches the task metadata | ✅ | ✅ | ❌ |
| Data and episode-metadata files exist and are readable | ✅ | ✅ | ✅ |
| Episode ids unique, contiguous, boundaries and global `index` valid | ✅ | ⚠️ | ✅ |
| Timestamps monotonic, match `fps`, no dropped-frame gaps | ✅ | ✅ | ✅ gap limit by config |
| `frame_index` sequential within each episode | ✅ | ✅ | ❌ |
| Declared dtype and shape match the stored data | ✅ float list widths | ⚠️ | ✅ |
| Same features in every episode (missing, extra, dtype, shape) | ✅ | ✅ | ⚠️ |
| Required streams present (`observation.state`, ...) | ⚠️ `action`, `observation.state`, `timestamp` | ❌ | ⚠️ needs config |
| Streams cover whole episodes; observation/action delay | ❌ | ❌ | ✅ delay by config |
| NaN and Inf in feature columns | ✅ | ✅ | ✅ |
| Numeric limits per feature | ✅ by config | ❌ | ⚠️ needs config |
| Implausible jumps between frames | ✅ also `max_step` by config | ✅ | ⚠️ needs config |
| Constant or frozen action, idle episodes | ✅ | ✅ | ❌ |
| Stuck or constant dimensions, zero variance | ✅ | ✅ | ❌ |
| Values clipped at their min or max | ✅ 5% of frames or more, configurable | ✅ | ❌ |
| Extreme outliers, distribution shift across episodes | ✅ | ✅ | ❌ |
| Duplicate episodes | ✅ exact action sequences | ✅ near-duplicates | ✅ exact |
| Episode length: empty, too short, outliers | ✅ | ✅ | ❌ |
| Statistics file present and covering every feature | ✅ | ✅ | ❌ |
| Statistics match the data (count, min, max, mean, std) | ✅ `stats.json`, v3.x and v2.0 | ❌ compares feature names only | ❌ |
| Video file exists, readable mp4, frame rate | ✅ | ✅ | ✅ |
| Video frame count against the episode (container tables) | ✅ | ✅ | ❌ |
| Video resolution against `info.json` | ✅ | ✅ | ❌ |
| Frames decode, frozen frames, black frames | ✅ opt-in `--decode` (H.264 and AV1) | ⚠️ first frame only | ✅ |
| Portability: absolute paths, symlinks, stray files in `data/`, oversized files | ✅ | ✅ | ❌ |
| Training readiness: zero-std statistics, episodes shorter than the policy chunk | ✅ | ✅ | ❌ |
| Only leCop: v3 video span checks, `range_*`, `index_mismatch` | ✅ | ❌ | ❌ |

Gaps are tracked as issues by priority; see the open issues labelled `feature`.

## Performance

Video is where the tools differ most: lerobot-doctor decodes the first frame of each video, physlint decodes every
frame, and leCop reads container metadata only unless asked. So two tables are reported, each giving the tools compared
the same video work: none decodes in the first, leCop and physlint decode everything in the second. Median wall time of 3 runs after one warm-up, warm page cache, including process start, every run in
a fresh empty working directory with physlint's result cache off. Apple M2 Pro, Python 3.12, leCop `release` build,
lerobot-doctor 0.2.0 and physlint 0.4.0 installed from PyPI. physlint reads v3 only. The tools check different things
(see the comparison above), so this compares speed only. Reproduce with `scripts/compare.py`;
`scripts/speed_chart.py` draws the charts from the tables below. The time axis is logarithmic.

### Without video decoding

lerobot-doctor runs every check but `videos`; physlint runs with its `video.decode`, `video.frozen_frames` and
`video.black_frames` rules off; leCop is the default build.

![Wall time per dataset without video decoding, log scale: leCop takes 8 to 28 ms, lerobot-doctor 368 to 686 ms, physlint 0.7 to 15 s](../assets/speed-novideo.svg)

| Dataset | leCop | lerobot-doctor | physlint | leCop vs the fastest other tool |
|---|---|---|---|---|
| `pusht` (v2.1) | 🏆 **19 ms** | 686 ms | n/a | 36x |
| `svla_so101_pickplace` (v2.1) | 🏆 **11 ms** | 444 ms | n/a | 40x |
| `c_careattrs_mug2v2` (v2.1) | 🏆 **8 ms** | 368 ms | n/a | 47x |
| `v3_pusht` (v3.0) | 🏆 **11 ms** | 596 ms | 15,408 ms | 54x |
| `v3_aloha_sim_insertion_human` (v3.0) | 🏆 **9 ms** | 435 ms | 2,742 ms | 50x |
| `v3_aloha_sim_transfer_cube_human` (v3.0) | 🏆 **9 ms** | 412 ms | 3,213 ms | 44x |
| `c_so101_pill_sorting_medoc` (v3.0) | 🏆 **28 ms** | 417 ms | 685 ms | 15x |
| `c_so101_my_task_50ep` (v3.0) | 🏆 **18 ms** | 460 ms | 7,700 ms | 25x |
| `c_so101_block_box_50episodes_final` (v3.0) | 🏆 **12 ms** | 377 ms | 2,774 ms | 31x |
| `c_black-stressball-tutorial` (v3.0) | 🏆 **14 ms** | 454 ms | 8,582 ms | 32x |

### With video decoding

leCop is built with `--features decode` and run with `--decode`; physlint runs all its rules. Both decode every frame.
Only physlint is compared here, and only on the v3 datasets it reads: lerobot-doctor decodes one frame per video, which
is not the same work.

![Wall time per dataset with video decoding, log scale: leCop takes 0.2 to 3.2 s, physlint 5 to 21 s](../assets/speed-video.svg)

| Dataset | leCop | physlint | leCop speedup |
|---|---|---|---|
| `v3_pusht` (v3.0) | 🏆 **185 ms** | 21,095 ms | 114x faster |
| `v3_aloha_sim_insertion_human` (v3.0) | 🏆 **2,585 ms** | 11,517 ms | 4.5x faster |
| `v3_aloha_sim_transfer_cube_human` (v3.0) | 🏆 **2,049 ms** | 10,226 ms | 5x faster |
| `c_so101_pill_sorting_medoc` (v3.0) | 🏆 **1,503 ms** | 5,190 ms | 3.5x faster |
| `c_so101_my_task_50ep` (v3.0) | 🏆 **2,664 ms** | 19,661 ms | 7.4x faster |
| `c_so101_block_box_50episodes_final` (v3.0) | 🏆 **1,291 ms** | 5,877 ms | 4.6x faster |
| `c_black-stressball-tutorial` (v3.0) | 🏆 **3,164 ms** | 17,162 ms | 5.4x faster |

leCop splits each video at its keyframes and decodes the pieces in parallel, so one large file uses every core. AV1 is
decoded with rav1d's hand-written assembly, 3 to 4 times faster than the same decoder without it on ARM (18 s against 67 s on
`c_so100_press_red_button`, Apple M-series) and 4.6 to 14 times on x86_64 (65 s against 296 s, AMD EPYC 7763). Every install has it on ARM and on x86_64 Linux, Windows and macOS,
including `cargo install lecop`, with no ffmpeg and no assembler on the machine (see
[Decoding frames](usage.md#decoding-frames)).

### A large dataset: RoboTwin

The datasets above are all fairly small, so we wanted to see what happens on a big one. We picked
[RoboTwin](https://huggingface.co/datasets/lerobot/robotwin_unified), a public dataset on the Hugging Face hub with
27,500 episodes, just over 6 million frames and three cameras, 75 GB on disk (v3.0 at commit `1287871`, 30 fps, AV1
video at 640x480, 289 MB of parquet and the rest video).

We ran the three tools one after the other on a workstation (AMD Ryzen 9 7900X with 24 threads, 124 GB RAM, Linux,
warm page cache, default thread count; leCop 0.3.0 installed with `cargo install lecop --locked` and no nasm, Rust
1.99.0; lerobot-doctor 0.2.0 and physlint 0.4.0 from PyPI on Python 3.14.3; same settings as `scripts/compare.py`,
times and peak memory from `/usr/bin/time -v`). Here is what we got:

| Run | leCop | lerobot-doctor | physlint |
|---|---|---|---|
| Without video decoding | 🏆 **0.73 s**, 1.8 GB | 50 min, 5.5 GB | 4 h 42 min, 1.1 GB |
| Decoding every frame | 🏆 **11 min 32 s**, 15 GB | n/a | n/a |

The leCop times are medians (3 runs without decoding, 2 with, each after a warm-up), with another job using under one
core in the background. The other two are a single run each on an otherwise quiet machine, since one physlint run
takes almost five hours (an earlier run of each, with a desktop file indexer busy in the background, took 8% and 10%
longer). physlint's decoding run read no frames from these AV1 videos, so there is no number for it in the second row.

### Why leCop is fast

- **Every pass over the data is linear.** leCop reads each data file once and checks all of its episodes from that
  one read. It finds duplicate episodes by hashing each episode's actions, not by comparing every pair, so its cost
  grows with the size of the dataset and no faster.
- **Video is read from the container, not decoded.** leCop's own mp4 parser seeks past the frame data and reads only
  the `moov` sample tables, so checking a video costs the same whatever its size. Without `--decode`, none of
  RoboTwin's 74 GB of video frames is read.
- **Work runs on every core.** Data files and videos are independent, so leCop checks them in parallel. With
  `--decode`, each video is also split at its keyframes into segments decoded in parallel, so even one large file uses
  every core. On RoboTwin leCop averaged four cores without decoding and all 24 when decoding (2353% CPU).
- **Compiled Rust, with assembly where it counts.** There is no interpreter to start: leCop starts in 1 ms. Columns
  are read as typed Arrow arrays and checked in compiled loops, built with link-time optimization. AV1 frames are
  decoded with rav1d's hand-written SIMD assembly, 4.6 to 14 times faster than without it on x86_64
  ([figures](usage.md#decoding-frames)).
