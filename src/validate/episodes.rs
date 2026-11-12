use super::jump_shift::{
    EpisodeNumbers, SHIFT_STRIDE, column_rows, in_quarter, is_shift_key, moments, step_moments,
};
use super::numeric::{DimStats, dim_stats, is_analyzed};
use super::schema::{FileData, Run, episode_rows, episode_runs, parse_file};
use super::{Issue, PHASE_DATA, Severity, Tagged, tag};
use crate::dataset::Dataset;
use crate::dataset::Episode;
use std::collections::BTreeSet;
use std::collections::HashMap;

/// Timestamps may deviate from frame_index/fps by this much (LeRobot's default tolerance).
pub(super) const TIMESTAMP_TOL_S: f64 = 1e-4;

/// An episode is reported when more than this fraction of its frames repeat the previous action.
// ponytail: fixed 50% threshold, from one so101 dataset where normal episodes idled < 50%; make it a flag if people disagree.
pub(super) const IDLE_FRACTION: f64 = 0.5;

/// Seconds the action may stay identical mid-episode before it is reported.
pub(super) const DEFAULT_FROZEN_SECONDS: f64 = 3.0;

/// How much of an action trajectory is dead time: transitions where the action vector did not change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Idle {
    pub transitions: usize,
    pub idle: usize,
    /// First and last frame whose action differs from the previous frame.
    pub first_move: Option<usize>,
    pub last_move: Option<usize>,
    /// The longest stretch of repeated rows between the first and the last change: how many transitions it spans,
    /// and the frame where it starts. Rest at the start and the end of the episode is not counted.
    pub longest_hold: usize,
    pub hold_from: usize,
}

/// `actions` is `rows * width` values, row-major. Exact equality is intended: real teleop noise is never
/// bit-identical, so a repeated row means the recording held the last command.
pub fn idle_profile(actions: &[f64], width: usize) -> Idle {
    let mut r = Idle::default();
    if width == 0 {
        return r;
    }
    let mut prev: Option<&[f64]> = None;
    let mut run_from: Option<usize> = None;
    for (i, row) in actions.chunks_exact(width).enumerate() {
        if let Some(p) = prev {
            r.transitions += 1;
            if p == row {
                r.idle += 1;
            } else {
                r.first_move.get_or_insert(i);
                r.last_move = Some(i);
                if let Some(c) = run_from.filter(|&c| i - c - 1 > r.longest_hold) {
                    r.longest_hold = i - c - 1;
                    r.hold_from = c;
                }
                run_from = Some(i);
            }
        }
        prev = Some(row);
    }
    r
}

/// The feature the action checks run on: `action`, or `actions` (seen in the wild). Not any `action*`
/// name: siblings such as `action_is_pad` are not the action.
pub(super) fn action_key(ds: &Dataset) -> Option<&str> {
    let f = &ds.info.features;
    ["action", "actions"].into_iter().find(|k| {
        f.get(*k)
            .is_some_and(|v| v.dtype == "float32" || v.dtype == "float64")
    })
}

/// (position, episode index, length, hash of its action rows)
pub(super) type ActionHash = (usize, usize, usize, u64);

/// What a data file contributes to the dataset-wide checks.
#[derive(Default)]
pub(super) struct GroupExtra {
    pub(super) episodes: Vec<EpisodeNumbers>,
    pub(super) tasks: BTreeSet<i64>,
    pub(super) hashes: Vec<ActionHash>,
    pub(super) stats: Vec<(String, Vec<DimStats>)>,
}

/// A warning on every episode whose action sequence equals that of an earlier one.
pub(super) fn duplicate_episodes(hashes: &[ActionHash]) -> Vec<Tagged> {
    let mut first: HashMap<(usize, u64), usize> = HashMap::new();
    let mut out = Vec::new();
    for &(pos, index, len, h) in hashes {
        match first.entry((len, h)) {
            std::collections::hash_map::Entry::Occupied(o) => out.push(tag(
                Some(pos),
                PHASE_DATA,
                Severity::Warning,
                Some(index),
                "duplicate_episode",
                format!("same action sequence as episode {}", o.get()),
            )),
            std::collections::hash_map::Entry::Vacant(v) => {
                v.insert(index);
            }
        }
    }
    out
}

/// Episodes shorter than this many frames are suspect: one second, and never fewer than 5 frames.
pub(super) fn short_threshold(fps: f64) -> usize {
    (fps.ceil() as usize).max(5)
}

/// Findings on episode lengths alone: `(position, severity, code, message)`. Outliers are judged only with 10 or
/// more episodes, where a mean and standard deviation mean something.
pub fn length_findings(
    lengths: &[usize],
    fps: f64,
) -> Vec<(usize, Severity, &'static str, String)> {
    let mut out = Vec::new();
    let threshold = short_threshold(fps);
    let n = lengths.len() as f64;
    let mean = lengths.iter().sum::<usize>() as f64 / n;
    let std = (lengths
        .iter()
        .map(|l| (*l as f64 - mean).powi(2))
        .sum::<f64>()
        / n)
        .sqrt();
    for (i, &len) in lengths.iter().enumerate() {
        if len == 0 {
            out.push((
                i,
                Severity::Error,
                "episode_empty",
                "episode has 0 frames".into(),
            ));
        } else if len < threshold {
            out.push((
                i,
                Severity::Warning,
                "episode_short",
                format!("{len} frames, under one second ({threshold} frames at {fps} fps)"),
            ));
        } else if lengths.len() >= 10 && std > 0.0 && (len as f64 - mean).abs() > 3.0 * std {
            out.push((
                i,
                Severity::Warning,
                "episode_length_outlier",
                format!(
                    "{len} frames, much {} than the other episodes (mean {mean:.0}, std {std:.0} over {} episodes)",
                    if len as f64 > mean { "longer" } else { "shorter" },
                    lengths.len()
                ),
            ));
        }
    }
    out
}

/// Order-sensitive hash of float rows by bit pattern. Four independent FxHash-style lanes: one serial chain was
/// ~12% of `validate` on the 600k-frame synthetic set (criterion), four lanes let the CPU overlap the multiplies.
// ponytail: no equality check after a hash match (a collision also needs an equal row count); add one if a false
// duplicate is ever reported.
pub fn rows_hash(vals: &[f64]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mix = |h: u64, x: f64| (h.rotate_left(5) ^ x.to_bits()).wrapping_mul(K);
    let mut lanes = [0u64; 4];
    let (chunks, rest) = vals.as_chunks::<4>();
    for c in chunks {
        for (l, x) in lanes.iter_mut().zip(c) {
            *l = mix(*l, *x);
        }
    }
    let h = lanes
        .iter()
        .fold(0u64, |h, l| (h.rotate_left(5) ^ l).wrapping_mul(K));
    rest.iter().fold(h, |h, x| mix(h, *x))
}

/// A warning when episodes are too short to hold one training window of `chunk` frames.
pub(super) fn chunk_finding(lengths: &[usize], chunk: usize) -> Option<String> {
    let short = lengths.iter().filter(|&&l| l < chunk).count();
    (short > 0).then(|| {
        format!(
            "{short} of {} episodes have fewer than {chunk} frames, the chunk size in lecop.toml",
            lengths.len()
        )
    })
}

/// Load one data file and check every episode stored in it. `eps` all share `eps[0].data_file`.
pub(super) fn check_group(
    ds: &Dataset,
    action: Option<&str>,
    start: usize,
    eps: &[Episode],
    frozen_seconds: f64,
) -> (Vec<Tagged>, GroupExtra) {
    use Severity::Error;
    let first = &eps[0];
    let at_first = |code, message: String| {
        (
            vec![tag(
                Some(start),
                PHASE_DATA,
                Error,
                Some(first.index),
                code,
                message,
            )],
            GroupExtra::default(),
        )
    };
    // One read syscall for the whole file: parquet's own reader issues many small preads, which stopped
    // scaling past ~3 threads (synthetic_v2 jobs10: 98 ms with File, 42 ms with this).
    let started = std::time::Instant::now();
    let Ok(bytes) = std::fs::read(&first.data_file) else {
        tracing::debug!(file = %first.data_file.display(), "data file missing");
        return at_first("data_missing", format!("{}", first.data_file.display()));
    };
    let size = bytes.len();
    let parsed = parse_file(bytes.into(), ds);
    tracing::debug!(
        file = %first.data_file.display(),
        bytes = size,
        episodes = eps.len(),
        elapsed_ms = started.elapsed().as_secs_f64() * 1e3,
        "read data file"
    );
    let d = match parsed {
        Ok(d) if d.ts.len() != d.rows => {
            return at_first(
                "timestamp_missing",
                format!(
                    "{}: `timestamp` column missing or not a float",
                    first.data_file.display()
                ),
            );
        }
        Ok(d) => d,
        Err(err) => return at_first("data_unreadable", format!("{err:#}")),
    };
    let runs = episode_runs(&d);
    let schema = d.schema.iter().map(|(sev, code, msg)| {
        tag(
            Some(start),
            PHASE_DATA,
            *sev,
            Some(first.index),
            code,
            msg.clone(),
        )
    });
    let per_episode = eps.iter().enumerate().flat_map(|(i, e)| {
        check_episode(ds, action, e, &d, &runs, frozen_seconds)
            .into_iter()
            .map(move |x| (Some(start + i), PHASE_DATA, x))
    });
    let stats: Vec<(String, Vec<DimStats>)> = d
        .floats
        .iter()
        .filter(|(k, _)| is_analyzed(k, action))
        .map(|(k, (vals, w))| (k.clone(), dim_stats(vals, *w)))
        .collect();
    let total = ds.episodes.len();
    let episodes = eps
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let (s, t) = episode_rows(&d, &runs, e.index)?;
            let slice = |col| column_rows(col, s, t);
            let moments = d
                .floats
                .iter()
                .filter(|(k, _)| in_quarter(start + i, total) && is_shift_key(k, action))
                .filter_map(|(k, col)| {
                    let (v, w) = slice(col)?;
                    if v.is_empty() {
                        return None;
                    }
                    Some((k.clone(), moments(v, w, SHIFT_STRIDE)))
                })
                .collect();
            let steps = action
                .and_then(|a| d.floats.get(a))
                .and_then(slice)
                .and_then(|(v, w)| step_moments(v, w));
            Some(EpisodeNumbers {
                pos: start + i,
                moments,
                steps,
            })
        })
        .collect();
    let hashes = eps
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let (vals, w) = d.floats.get(action?)?;
            let (s, t) =
                d.ep.as_ref()
                    .map_or(Some((0, d.rows)), |_| runs.get(&(e.index as i64)).copied())?;
            let rows = vals.get(s * w..t * w)?;
            (!rows.is_empty()).then(|| (start + i, e.index, e.length, rows_hash(rows)))
        })
        .collect();
    (
        schema.chain(per_episode).collect(),
        GroupExtra {
            episodes,
            tasks: d.tasks.clone(),
            hashes,
            stats,
        },
    )
}

/// Where a global `index` column stops counting up from `from`: its position in the episode and the value found.
fn first_index_break(idx: &[i64], from: i64) -> Option<(usize, i64)> {
    idx.iter()
        .enumerate()
        .find(|(i, x)| **x != from + *i as i64)
        .map(|(i, x)| (i, *x))
}

pub(super) fn check_episode(
    ds: &Dataset,
    action: Option<&str>,
    e: &Episode,
    d: &FileData,
    runs: &HashMap<i64, Run>,
    frozen_seconds: f64,
) -> Vec<Issue> {
    use Severity::*;
    let mut v = Vec::new();
    let mut push = |s, code, message: String| {
        v.push(Issue {
            severity: s,
            episode: Some(e.index),
            code,
            message,
        })
    };
    // Built only for a finding: a String per episode on a healthy dataset showed up in the per-episode loop.
    let file = || {
        e.data_file
            .strip_prefix(&ds.root)
            .unwrap_or(&e.data_file)
            .display()
            .to_string()
    };

    // This episode's rows: its run in a packed file, or the whole file if there is no episode_index column.
    let (s, t) = match &d.ep {
        Some(_) => match runs.get(&(e.index as i64)) {
            Some(r) => *r,
            None => {
                push(
                    Error,
                    "episode_rows_missing",
                    format!("{} has no rows with episode_index {}", file(), e.index),
                );
                return v;
            }
        },
        None => (0, d.ts.len()),
    };
    if t - s != e.length {
        push(
            Error,
            "length_mismatch",
            format!(
                "metadata says {} frames, {} has {} rows for this episode",
                e.length,
                file(),
                t - s
            ),
        );
    }
    if let (Some((from, _)), Some(idx)) = (e.range, &d.idx)
        && let Some((i, found)) = first_index_break(&idx[s..t], from)
    {
        push(
            Error,
            "index_mismatch",
            format!(
                "{}: the global `index` column should count up from {from} through this episode, but frame {i} reads {found} instead of {}",
                file(),
                from + i as i64
            ),
        );
    }
    let ts = &d.ts[s..t];
    if let Some(i) = ts.windows(2).position(|w| w[1] <= w[0]) {
        push(
            Error,
            "timestamp_not_monotonic",
            format!("at frame {} ({} -> {})", i + 1, ts[i], ts[i + 1]),
        );
    }
    if let Some((i, x)) = ts
        .iter()
        .enumerate()
        .find(|(i, x)| (**x - *i as f64 / ds.info.fps).abs() > TIMESTAMP_TOL_S)
    {
        push(
            Error,
            "timestamp_drift",
            format!(
                "frame {i}: t={x:.5}s, expected {:.5}s at {} fps",
                i as f64 / ds.info.fps,
                ds.info.fps
            ),
        );
    }
    if !d.fi.is_empty() && d.fi[s..t].iter().enumerate().any(|(i, f)| *f != i as i64) {
        push(Error, "frame_index", "frame_index is not 0..n-1".into());
    }
    for (k, (vals, w)) in &d.floats {
        let Some(slice) = vals.get(s * w..t * w) else {
            push(
                Error,
                "column_shape",
                format!(
                    "{k}: {} values do not fit {} rows of width {w}",
                    vals.len(),
                    d.rows
                ),
            );
            continue;
        };
        let n = slice.iter().filter(|x| !x.is_finite()).count();
        if n > 0 {
            push(Error, "non_finite", format!("{k}: {n} NaN/Inf values"));
        }
        if Some(k.as_str()) == action && *w > 0 && slice.len() > *w {
            let idle = idle_profile(slice, *w);
            if idle.idle == idle.transitions {
                push(
                    Warning,
                    "action_constant",
                    "action is identical on every frame".into(),
                );
            } else if idle.idle as f64 > IDLE_FRACTION * idle.transitions as f64 {
                push(
                    Warning,
                    "idle_episode",
                    format!(
                        "{:.0}% of frames repeat the previous action; it first changes at frame {} and last at frame {} of {}",
                        100.0 * idle.idle as f64 / idle.transitions as f64,
                        idle.first_move.unwrap_or(0),
                        idle.last_move.unwrap_or(0),
                        idle.transitions + 1
                    ),
                );
            }
            let held = idle.longest_hold as f64 / ds.info.fps;
            if held >= frozen_seconds {
                push(
                    Warning,
                    "action_frozen",
                    format!(
                        "action is identical for {held:.1} s ({} frames) from frame {}",
                        idle.longest_hold + 1,
                        idle.hold_from
                    ),
                );
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_where_the_global_index_breaks() {
        assert_eq!(first_index_break(&[10, 11, 12], 10), None);
        assert_eq!(first_index_break(&[10, 11, 14, 15], 10), Some((2, 14)));
        assert_eq!(first_index_break(&[], 10), None);
    }

    #[test]
    fn idle_profile_counts_repeated_rows_and_first_last_change() {
        // width 2, rows: A A A B B C  -> transitions: same same diff same diff
        let a = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 2.0, 3.0, 3.0];
        let p = idle_profile(&a, 2);
        assert_eq!(
            p,
            Idle {
                transitions: 5,
                idle: 3,
                first_move: Some(3),
                last_move: Some(5),
                longest_hold: 1,
                hold_from: 3,
            }
        );
    }

    /// Rows with one value each, width 1.
    fn hold(rows: &[f64]) -> (usize, usize) {
        let p = idle_profile(rows, 1);
        (p.longest_hold, p.hold_from)
    }

    #[test]
    fn a_hold_is_the_longest_repeat_between_the_first_and_last_change() {
        // 1 2 2 2 2 2 3 3 4: the 2s repeat 4 times from frame 1, the 3s once.
        assert_eq!(hold(&[1.0, 2.0, 2.0, 2.0, 2.0, 2.0, 3.0, 3.0, 4.0]), (4, 1));
    }

    #[test]
    fn rest_at_the_start_and_the_end_is_not_a_hold() {
        assert_eq!(hold(&[1.0, 1.0, 1.0, 1.0, 2.0, 3.0, 3.0, 3.0, 3.0]), (0, 0));
    }

    #[test]
    fn a_later_hold_replaces_a_shorter_earlier_one() {
        assert_eq!(hold(&[1.0, 2.0, 2.0, 3.0, 4.0, 4.0, 4.0, 5.0]), (2, 4));
    }

    #[test]
    fn a_constant_or_single_change_action_has_no_hold() {
        assert_eq!(hold(&[1.0, 1.0, 1.0]), (0, 0));
        assert_eq!(hold(&[1.0, 1.0, 2.0, 2.0]), (0, 0));
    }

    #[test]
    fn idle_profile_handles_degenerate_input() {
        assert_eq!(idle_profile(&[], 3), Idle::default());
        assert_eq!(idle_profile(&[1.0, 2.0], 0), Idle::default()); // zero width must not panic
        assert_eq!(idle_profile(&[1.0, 2.0], 2).transitions, 0); // a single row has no transitions
    }
}
