# Usage

```bash
lecop inspect  path/to/dataset   # summary from metadata only
lecop .                          # same as `lecop validate .`: scan the current directory for datasets
lecop validate                   # scan the current directory for datasets
lecop validate path/to/dataset   # one dataset, or a directory that contains many
```

`validate` is the default command: `lecop <paths> [flags]` is `lecop validate <paths> [flags]`. `validate` takes any
number of paths. A path that holds `meta/info.json` is a dataset; any other directory is
searched for datasets (`.gitignore` is respected). Datasets are checked in parallel and reported in sorted order.

```
$ lecop validate datasets
datasets/c_so100_press_red_button: ERROR   ep 7      video_frame_count        observation.images.left has 591 frames, episode has 588
datasets/c_so101_my_task_50ep: WARNING ep 18     idle_episode             64% of frames repeat the previous action; ...
Found 22 errors and 6 warnings in 3 datasets.
```

A clean run prints `All checks passed!`. Exit code: 0 clean (warnings do not fail), 1 if any error, 2 if a dataset,
the config or an argument cannot be read. A dataset whose metadata cannot be loaded (missing or invalid `meta/info.json`,
unsupported version, `fps` not positive) is reported as `cannot_open`: none of its other checks run, and `--ignore`,
`--select` and the config cannot hide it.

| Option | |
|---|---|
| `--format text\|full\|json\|github` (alias `--output-format`) | `full` adds the location and a one-line explanation per finding; `json` is one object (`errors`, `warnings`, `ignored`, `issues[]`); `github` prints `::error`/`::warning` annotations |
| `--statistics` | findings per issue code |
| `-q`, `--quiet` | summary only, nothing on success |
| `--no-color` | colour is also off when piped or when `NO_COLOR` is set |
| `--ignore a,b`, `--select a,b` | drop, or keep only, these issue codes |
| `--decode` | decode the video frames and report undecodable, frozen and black cameras; needs a build that includes the decoders (the default), see below |
| `--fix`, `--preview` | apply the safe repairs, or show them without changing anything (`--diff` is an alias), see below |
| `--config FILE`, `--isolated` | use this config, or none |
| `-v`, `-vv` | log progress, then per-file timings, to stderr (never stdout); `RUST_LOG` overrides |
| `-j N` | worker threads (default: one per core); output is identical for every `-j` |

## Repairing

`validate` alone never writes. `--fix` (also `lecop . --fix`) applies the only repairs leCop makes: the totals in
`meta/info.json` recomputed from metadata it does not touch, then validates again. `--preview` shows what `--fix` would
change and writes nothing. When a safe repair exists, a plain run ends with `[*] 1 dataset fixable with --fix` on stderr.

```bash
lecop validate --preview datasets/my_dataset   # prints what would change, on stderr
lecop validate --fix datasets/my_dataset    # applies it
```

| Finding | Repair |
|---|---|
| `frame_count` | `total_frames` becomes the sum of the episode lengths |
| `episode_count` | `total_episodes` becomes the number of episodes in the metadata |
| `tasks_count` | `total_tasks` becomes the number of tasks in the task metadata |

Nothing is repaired when the episode metadata and the data files disagree (`length_mismatch`, `episode_rows_missing`,
`data_missing`, `data_unreadable`, `timestamp_missing`, `episode_index`, `range_length`, `range_gap`, `index_mismatch`): a
total taken from that metadata could be as wrong as the one it replaces. Only the number changes, key order and
formatting stay. The original is kept as `meta/info.json.bak`, and `--fix` stops if that file already exists. Data
and video files are never edited.

## Decoding frames

By default leCop never decodes video: it reads container metadata only, which is why it takes milliseconds. Every
build includes the decoders; pass `--decode` to use them:

```bash
lecop validate --decode datasets/my_dataset
```

The PyPI wheels, the release binaries and `cargo binstall lecop` are built with the `asm` feature: rav1d's hand-written
assembly, which decodes AV1 about 3 times faster (19 s against 60 s on `c_so100_press_red_button`, Apple M-series).
`cargo install lecop` builds without it so that it needs no extra tools. To get the fast build from source, install
[nasm](https://www.nasm.us) (x86_64 only; `apt install nasm`, `brew install nasm`; ARM builds use the C compiler) and run
`cargo install lecop --features asm`. `cargo install lecop --no-default-features` builds without the decoders (9.1 MB
instead of 11.2 MB); `--decode` then says it needs a build with `--features decode`.

H.264 is decoded with openh264 and AV1 with rav1d (the `hiraku-rav1d` fork). Per frame leCop keeps the mean
luma and the difference to the previous frame, so it reports frames the decoder rejects (`video_decode`), a camera that
repeats the same picture (`video_frozen`) and one that shows black (`video_black`). The output ends with
`Video frames were decoded (--decode).` when it ran. A video in another codec is reported as `not decoded`, not skipped.
Measured on an Apple M2 Pro, release build: 1.5 s for `c_so101_pill_sorting_medoc`, 2.5 s for `c_careattrs_mug2v2`, 17 s for
`c_so100_press_red_button` and 2.7 s for `c_so101_my_task_50ep` (AV1); 0.26 s for `c_d3il_avoiding_vision_224` (H.264). The
feature adds about 2.1 MB to the binary (11.2 MB against 9.1 MB).

## Config file

`lecop.toml` in the scanned directory or any parent (nearest wins):

```toml
ignore = ["idle_episode"]          # drop these codes
# select = ["frame_count"]         # or keep only these
# error = ["video_fps"]            # promote a warning to an error
# warn = ["frame_count"]           # demote an error to a warning

[per-dataset."datasets/messy"]     # path relative to this file
ignore = ["video_frame_count"]

[limits."action"]                  # only `action` and `observation.*` features
min = -1.0                         # a number, or one value per dimension: [-1.0, -0.5, ...]
max = [1.0, 1.0, 0.5]
max_step = 0.2                     # largest change of any action dimension between two frames

[training]
chunk_size = 100                   # frames a policy window spans; episodes shorter than this are reported

[numeric]
clipped_fraction = 0.05            # report a dimension with this fraction of its values (or more) on its min or max
frozen_seconds = 3.0               # report an action that stays identical this long (or longer) mid-episode
```

Unknown issue codes are an error. Ignored findings are counted: `Ignored by config: idle_episode (5)`.
