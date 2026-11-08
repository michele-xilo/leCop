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

## 2026-11-08 v3.0 support implemented
`Dataset` resolves every episode (v2 or v3) to data file, optional global row range and video files;
validator reads each packed data file once, one at a time (whole file in memory, ceiling noted in code).
v3-only checks: `range_length`, `range_gap`, `index_mismatch`. Timestamp assumption (episode-relative in
v3) verified on real data.

## 2026-11-08 Own moov-only mp4 parser instead of the `mp4` crate
Real LeRobot videos are AV1 with the `moov` box after a multi-MB `mdat`. The `mp4` crate may not handle
AV1 and reads more than needed. `src/mp4.rs` scans top-level box headers, seeks past `mdat`, reads only
`moov` and takes frame count from `stsz` and duration from summed `stts`; inconsistent `stts`/`stsz` is
reported as corruption. Cost is independent of video size (benchmarked). Known gap: v3 per-episode frame
counts inside a shared video are not checked (only the episode's [from, to) span); would need `stts`
sample times and care with B-frame composition offsets (`ctts`).

## 2026-11-08 Branch workflow and rules
Rules adapted from task-audit live in `.claude/rules/` (general, modus-operandi, TDD, performance).
Feature work goes on a branch; ask before pushing. Performance rules require criterion benches and
rayon parallelism; the validator is still sequential (planned as M4), so the rules are ahead of the code.

## 2026-11-08 Semantic checks: nonstandard_features, idle_episode
Added after running lecop on 7 community datasets found no structural faults (see m1-notes.md). Both
warnings. `idle_episode`: > 50% of transitions repeat the previous action row exactly (constant action
keeps its own `action_constant` code, not both). Threshold is a fixed constant from a single dataset; it is
a judgement call and should become a flag if anyone disagrees. Action checks use `action`, falling back to
`actions` (not any `action*`: `action_is_pad` exists in real datasets). Missing `observation.state` is not checked yet (the test fixtures would need
the column). On the 15 local real datasets the two checks fire only on the two datasets that motivated
them (1 naming warning, 5 idle warnings); the other 13 stay clean.

## 2026-11-08 License: MIT OR Apache-2.0
Rust convention. Copyright holder assumed "Michele Xiloyannis"; user should confirm.

## Unverified assumptions (check before relying)
- v2.1 schema details came from memory/web summaries, not LeRobot source. M1 tests this.
- mp4 frame count from the sample table may mislead with VFR/B-frame edit lists.
