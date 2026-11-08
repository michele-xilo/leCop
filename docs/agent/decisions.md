# Decision log

## 2026-11-08 Project name: `lecop`
Chosen over robofsck (read as a profanity), robopulse, trajlint, lelint. "Le" is a pun on LeRobot;
the name does not contain "lerobot" itself. README states it is unofficial. Cost: ties the brand to
LeRobot if other formats (MCAP, RLDS) are added later.

## 2026-11-08 Positioning
Python validators already exist (lerobot-doctor, physlint, rdq, lerobot-quality-gates, robovet, trajlens)
with little traction (0-5 stars; lerobot-doctor ~1.8k PyPI downloads/month). Not competing on check
count. Differentiators: speed (rayon), video checks from mp4 metadata without decoding, single static
binary. Treated as a credibility/portfolio project, not a product. Success signal in PLAN.md.

## 2026-11-08 v2.x first, v3 now urgent
MVP targeted LeRobot v2.x (jsonl metadata). Checking the Hub showed v3.0 is the default for most
popular datasets, including small official `lerobot/*` ones. lecop currently rejects v3. Decision:
move v3 support ahead of video checks (M3 before M2). `lerobot/*` repos keep a `v2.1` revision
usable for v2.1 testing. v3 field names are still unverified; read LeRobot source before coding.

## 2026-11-08 License: MIT OR Apache-2.0
Rust convention. Copyright holder assumed "Michele Xiloyannis"; user should confirm.

## Unverified assumptions (check before relying)
- v2.1 schema details came from memory/web summaries, not LeRobot source. M1 tests this.
- mp4 frame count from the sample table may mislead with VFR/B-frame edit lists.
