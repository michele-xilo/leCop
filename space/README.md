---
title: leCop
emoji: 🤖
colorFrom: blue
colorTo: gray
sdk: gradio
sdk_version: 6.32.0
app_file: app.py
pinned: false
license: mit
short_description: Fast health checks for LeRobot datasets on the Hub
---

# leCop on a Hub dataset

Pick a public LeRobot dataset on the Hub and get [leCop](https://github.com/michele-xilo/lecop)'s report, grouped by
issue code in the look of the [results site](https://michele-xilo.github.io/leCop/). The dataset box filters the 2,000
most downloaded datasets the Hub tags LeRobot (gated ones left out) plus those on the results site; any other
`owner/name`, `owner/name@revision` or Hub URL can be typed in full.

The datasets that fit the Space's limits are listed first, the ones over a limit last with the reason, and repos with
no `meta/info.json` are left out. That order comes from `fits.json`, built by `build_fits.py` at each deploy (it needs a
Hugging Face login: the anonymous rate limit is too low for the few thousand requests), and every check run on the
Space updates it in memory. A `fits.json` built for other limits is ignored.

- The Space downloads `meta/` and `data/` only, so the video checks are skipped. Run `lecop validate` locally on a
  full download to check the videos too.
- Public, ungated datasets only. The Space uses no token.
- `meta/` and `data/` together must hold at most 2,000 files (`LECOP_SPACE_MAX_FILES`) and 2 GB
  (`LECOP_SPACE_MAX_BYTES`), and leCop gets 300 s (`LECOP_SPACE_TIMEOUT_S`). Over a limit, the page says so. The file cap
  is the one most datasets meet first: v2.x stores one parquet file per episode, and the Hub serves anonymous downloads at
  about 13 files a second.

## Deploying

`.github/workflows/space.yml` deploys this folder after every PyPI release of a stable version, on pushes to `main` that
change `space/`, and by hand (Actions, Space, Run workflow). It pins the newest `lecop` on PyPI in `requirements.txt`,
runs the unit tests, rebuilds `fits.json`, checks `lerobot/pusht` with the app, uploads, and waits until the Space runs
the new commit. It needs an `HF_TOKEN` secret on the `huggingface` environment: a Hugging Face token with write access to
the Space.

The logic outside the UI is in `lecop_space.py`, tested by `python3 -m unittest discover -s space`, and the Hub queries
are in `hub.py`. To deploy by hand, from the repository root, with a Hugging Face login:

```bash
cd space && uv run --with-requirements requirements.txt python build_fits.py
hf upload michele-xilo/lecop . --repo-type space --exclude "__pycache__/*" --exclude "test_*.py" --exclude build_fits.py
```
