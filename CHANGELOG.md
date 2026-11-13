# Changelog

## Unreleased

The PyPI wheel installs a `lecop` Python module: `lecop.validate(path)` runs the bundled binary and returns the
`--format json` report as a dict. The `lecop` command is unchanged, still the native binary.

The crate documentation has runnable examples and states what is stable: the issue codes, the CLI output and the exit
codes. The Rust API can change in any 0.x release.

## 0.1.2

rav1d's assembly moves behind an `asm` feature, off by default, so `cargo install lecop` no longer needs nasm on x86_64.
The PyPI wheels, release binaries and `cargo binstall lecop` are built with it. 0.1.1 reached PyPI but its GitHub
Release build failed on Windows, so the 0.1.2 release is the first with 0.1.1's decoder change in the binaries.

## 0.1.1

`--decode` works in every build, including the PyPI wheels and release binaries: the decoders are now a default
feature (+2.1 MB). Frames are still only decoded when `--decode` is passed. `--no-default-features` leaves them out.

## 0.1.0

First release. Health checks for LeRobot v2.x and v3.x datasets: metadata and totals, timestamps, files, video
container metadata, action and state signals, statistics, and portability. Output as text, full, JSON or GitHub
annotations; `lecop.toml` to ignore, re-grade or limit findings; `--fix` for the totals in `meta/info.json`;
opt-in video decoding with `--decode`. See [docs/checks.md](docs/checks.md) for every issue code.
