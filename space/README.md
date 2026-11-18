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

- The Space downloads `meta/` and `data/` only, so the video checks are skipped. Run `lecop validate` locally on a
  full download to check the videos too.
- Public, ungated datasets only. The Space uses no token.
- `meta/` and `data/` together must fit in 2 GB (`LECOP_SPACE_MAX_BYTES`), and leCop gets 300 s
  (`LECOP_SPACE_TIMEOUT_S`). Over the limit, the page says so.

The Space runs the `lecop` wheel pinned in `requirements.txt`. Its logic outside the UI is in `lecop_space.py`, tested by
`python3 -m unittest discover -s space`.
