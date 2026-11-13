<p align="center">
  <img src="assets/logo.png" alt="leCop: a robot cop with sunglasses" width="160">
</p>

<h1 align="center">leCop</h1>

<p align="center">
  <a href="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml"><img src="https://github.com/michele-xilo/lecop/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://app.codecov.io/gh/michele-xilo/leCop"><img src="https://codecov.io/gh/michele-xilo/leCop/graph/badge.svg" alt="codecov"></a>
  <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue" alt="license: MIT OR Apache-2.0">
  <img src="https://img.shields.io/badge/rust-stable-orange?logo=rust" alt="Rust">
</p>

**Find the broken episodes in your [LeRobot](https://github.com/huggingface/lerobot) dataset in milliseconds, before they cost you a training run.**

<p align="center">
  <img src="assets/demo.gif" alt="lecop validating a dataset: it lists a video with the wrong number of frames and an idle episode" width="800">
</p>

## Quick start

Install it with the tool you already use:

<details open>
<summary><b>uv</b></summary>

```bash
uv tool install lecop
```

Or run it once without installing: `uvx lecop path/to/dataset`.

</details>

<details>
<summary><b>pip</b></summary>

```bash
pip install lecop      # or: pipx install lecop
```

</details>

<details>
<summary><b>cargo</b></summary>

```bash
cargo binstall lecop   # prebuilt binary, nothing to compile
```

Or `cargo install lecop` to compile it yourself. That build needs no extra tools, but decodes AV1 (`--decode`) about
3 times slower; for the fast build install [nasm](https://www.nasm.us) (x86_64 only: `apt install nasm`,
`brew install nasm`) and run `cargo install lecop --features asm`.

</details>

Then run it on a dataset:

```bash
lecop path/to/dataset
```

No config, no dataset download, and the wheel is a single Rust binary: no Python dependencies. Exit code 0 means
clean, 1 means errors, so it works as a step in a data pipeline (see [In a pipeline](docs/usage.md#in-a-pipeline)). The PyPI wheels, `cargo binstall` and the
[release binaries](https://github.com/michele-xilo/lecop/releases) are built with the AV1 assembly; everything except
`--decode` runs at the same speed in every build.

### From Python

leCop is a command-line tool, not an importable module. Call it and read the JSON:

```python
import json, subprocess

run = subprocess.run(["lecop", "validate", "--format", "json", "path/to/dataset"], capture_output=True, text=True)
report = json.loads(run.stdout)
print(report["errors"], report["warnings"])
for issue in report["issues"]:
    print(issue)
```

## What it catches

- A video with 591 frames for an episode of 588.
- Timestamps that drift, or frames that went missing.
- Totals in `info.json` that disagree with the data (and `--fix` repairs them).
- A robot that sat idle for most of an episode, or whose action froze for seconds.
- Statistics that do not match the data, constant or clipped signals, NaNs.
- Missing, truncated or unreadable files.
- With `--decode`: a camera that froze or went black.

58 checks in all, listed in [docs/checks.md](docs/checks.md).

## Why leCop

- **Fast.** 8 to 28 ms per dataset without video decoding, 15x to 54x faster than lerobot-doctor on the same checks.
  Video is checked from the container, not by decoding frames, unless you ask for `--decode`.
- **Nothing to set up.** One binary. LeRobot v2.x and v3.x.
- **Made for pipelines.** Run it after recording or conversion, or as a gate before training. Stable issue codes, `--format json` and GitHub annotations, a config file to ignore or re-grade
  findings.

Compared with lerobot-doctor and physlint, check by check, in [docs/comparison.md](docs/comparison.md).

## More

- [Usage](docs/usage.md): every flag, output formats, `--fix`, `--decode`, `lecop.toml`.
- [Checks](docs/checks.md): every issue code and what is not supported yet.
- [Comparison and speed tables](docs/comparison.md).
- Open work is tracked in the [issues](https://github.com/michele-xilo/lecop/issues), by priority.

> **Unofficial.** Not affiliated with Hugging Face or the LeRobot project. Early stage: tested on synthetic fixtures
> and on 16 real Hub datasets (v2.1 and v3.0), one of them with real faults; no real v2.0 dataset yet.

## Contributing

```bash
cargo test
cargo bench   # criterion; see .claude/rules/performance.md

scripts/fetch-datasets.sh          # list real test datasets that are missing
scripts/fetch-datasets.sh --yes    # download them into datasets/ (git-ignored)
```

`cargo test` also checks the error and warning counts on every downloaded dataset listed in
`scripts/datasets.tsv` and skips the ones you do not have.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [STYLE_GUIDE.md](STYLE_GUIDE.md).

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
