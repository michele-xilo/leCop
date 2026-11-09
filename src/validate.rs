use crate::{
    dataset::{Dataset, Episode},
    mp4::{VideoInfo, read_video_info},
};
use anyhow::{Context, Result};
use arrow_array::{Array, ArrayRef, cast::AsArray, types::*};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub episode: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

/// Timestamps may deviate from frame_index/fps by this much (LeRobot's default tolerance).
const TIMESTAMP_TOL_S: f64 = 1e-4;

/// Relative difference between a video's frame rate and info.json's that we still accept.
const VIDEO_FPS_TOL: f64 = 0.01;

/// An episode is reported when more than this fraction of its frames repeat the previous action.
// ponytail: fixed 50% threshold, from one so101 dataset where normal episodes idled < 50%; make it a flag if people disagree.
const IDLE_FRACTION: f64 = 0.5;

/// Row range of one episode inside a data file.
type Run = (usize, usize);

/// Outcome of reading one video file: its info, or (issue code, message).
type VideoRead = Result<VideoInfo, (&'static str, String)>;

/// One data parquet file in memory. v3 files hold many episodes.
// ponytail: whole file in memory, one file at a time. Fine for ~100 MB shards; stream by row group if files get huge.
#[derive(Default)]
struct FileData {
    rows: usize,
    ts: Vec<f64>,
    fi: Vec<i64>,
    ep: Option<Vec<i64>>,
    idx: Option<Vec<i64>>,
    floats: BTreeMap<String, (Vec<f64>, usize)>,
}

fn parse_file(bytes: bytes::Bytes, ds: &Dataset) -> Result<FileData> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(bytes)?.build()?;
    let mut d = FileData::default();
    for batch in reader {
        let batch = batch?;
        d.rows += batch.num_rows();
        if let Some(c) = batch.column_by_name("timestamp") {
            d.ts.extend(as_f64(c));
        }
        if let Some(c) = batch
            .column_by_name("frame_index")
            .and_then(|c| c.as_primitive_opt::<Int64Type>())
        {
            d.fi.extend(c.values().iter().copied());
        }
        if let Some(c) = batch
            .column_by_name("episode_index")
            .and_then(|c| c.as_primitive_opt::<Int64Type>())
        {
            d.ep.get_or_insert_with(Vec::new)
                .extend(c.values().iter().copied());
        }
        if let Some(c) = batch
            .column_by_name("index")
            .and_then(|c| c.as_primitive_opt::<Int64Type>())
        {
            d.idx
                .get_or_insert_with(Vec::new)
                .extend(c.values().iter().copied());
        }
        for (key, f) in &ds.info.features {
            if f.dtype != "float32" && f.dtype != "float64" {
                continue;
            }
            let Some((vals, w)) = batch.column_by_name(key).and_then(flatten) else {
                continue;
            };
            let e = d.floats.entry(key.clone()).or_insert((Vec::new(), w));
            e.0.extend(vals);
        }
    }
    Ok(d)
}

/// Row ranges per episode_index inside a file (episodes are stored contiguously).
fn episode_runs(d: &FileData) -> HashMap<i64, Run> {
    let mut runs = HashMap::new();
    let Some(ep) = &d.ep else { return runs };
    for (i, e) in ep.iter().enumerate() {
        runs.entry(*e)
            .and_modify(|r: &mut (usize, usize)| r.1 = i + 1)
            .or_insert((i, i + 1));
    }
    runs
}

/// How much of an action trajectory is dead time: transitions where the action vector did not change.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Idle {
    pub transitions: usize,
    pub idle: usize,
    /// First and last frame whose action differs from the previous frame.
    pub first_move: Option<usize>,
    pub last_move: Option<usize>,
}

/// `actions` is `rows * width` values, row-major. Exact equality is intended: real teleop noise is never
/// bit-identical, so a repeated row means the recording held the last command.
pub fn idle_profile(actions: &[f64], width: usize) -> Idle {
    let mut r = Idle::default();
    if width == 0 {
        return r;
    }
    let mut prev: Option<&[f64]> = None;
    for (i, row) in actions.chunks_exact(width).enumerate() {
        if let Some(p) = prev {
            r.transitions += 1;
            if p == row {
                r.idle += 1;
            } else {
                r.first_move.get_or_insert(i);
                r.last_move = Some(i);
            }
        }
        prev = Some(row);
    }
    r
}

/// The feature the action checks run on: `action`, or `actions` (seen in the wild). Not any `action*`
/// name: siblings such as `action_is_pad` are not the action.
fn action_key(ds: &Dataset) -> Option<&str> {
    let f = &ds.info.features;
    ["action", "actions"].into_iter().find(|k| {
        f.get(*k)
            .is_some_and(|v| v.dtype == "float32" || v.dtype == "float64")
    })
}

/// An issue plus where it sorts in the report: episode position (None = dataset-wide) and check phase.
type Tagged = (Option<usize>, u8, Issue);

const PHASE_META: u8 = 0;
const PHASE_VIDEO: u8 = 1;
const PHASE_DATA: u8 = 2;

fn tag(
    pos: Option<usize>,
    phase: u8,
    severity: Severity,
    episode: Option<usize>,
    code: &'static str,
    message: String,
) -> Tagged {
    (
        pos,
        phase,
        Issue {
            severity,
            episode,
            code,
            message,
        },
    )
}

/// Run all checks on the global rayon-default number of threads.
pub fn validate(ds: &Dataset) -> Result<Vec<Issue>> {
    validate_with(ds, 0)
}

/// Run all checks on `jobs` threads (0 = one per core). The report is identical for every `jobs`.
pub fn validate_with(ds: &Dataset, jobs: usize) -> Result<Vec<Issue>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .context("building thread pool")?;
    Ok(pool.install(|| run(ds)))
}

fn run(ds: &Dataset) -> Vec<Issue> {
    let action = action_key(ds);
    let mut tagged = global_issues(ds, action);

    let reads: HashMap<PathBuf, VideoRead> = unique_video_paths(ds)
        .into_par_iter()
        .map(|p| {
            let r = read_video(&p);
            (p, r)
        })
        .collect();
    tagged.extend(metadata_and_video_issues(ds, &reads));

    // One unit of work per run of episodes sharing a data file: a v2 file is one episode, a v3 file many.
    let mut groups: Vec<(usize, &[Episode])> = Vec::new();
    let mut start = 0;
    for g in ds.episodes.chunk_by(|a, b| a.data_file == b.data_file) {
        groups.push((start, g));
        start += g.len();
    }
    let data: Vec<Vec<Tagged>> = groups
        .into_par_iter()
        .map(|(start, eps)| check_group(ds, action, start, eps))
        .collect();
    tagged.extend(data.into_iter().flatten());

    // Stable sort: ties keep their within-phase order, so the report does not depend on thread scheduling.
    tagged.sort_by_key(|(pos, phase, _)| (*pos, *phase));
    // A data file missing from several separate runs of episodes is reported once.
    let mut seen = std::collections::HashSet::new();
    tagged.retain(|(_, _, i)| i.code != "data_missing" || seen.insert(i.message.clone()));
    tagged.into_iter().map(|(_, _, i)| i).collect()
}

fn global_issues(ds: &Dataset, action: Option<&str>) -> Vec<Tagged> {
    use Severity::*;
    let mut v = Vec::new();
    if !ds.info.features.contains_key("action") {
        let found = action.map_or(String::new(), |k| {
            format!("; running the action checks on `{k}`")
        });
        v.push(tag(
            None,
            PHASE_META,
            Warning,
            None,
            "nonstandard_features",
            format!("no `action` feature, which LeRobot policies expect{found}"),
        ));
    }
    if ds.episodes.len() != ds.info.total_episodes {
        v.push(tag(
            None,
            PHASE_META,
            Error,
            None,
            "episode_count",
            format!(
                "info.json says {} episodes, metadata has {}",
                ds.info.total_episodes,
                ds.episodes.len()
            ),
        ));
    }
    let sum: usize = ds.episodes.iter().map(|e| e.length).sum();
    if sum != ds.info.total_frames {
        v.push(tag(
            None,
            PHASE_META,
            Error,
            None,
            "frame_count",
            format!(
                "info.json says {} frames, episode lengths sum to {sum}",
                ds.info.total_frames
            ),
        ));
    }
    v
}

fn read_video(path: &Path) -> VideoRead {
    if !path.exists() {
        return Err(("video_missing", format!("{}", path.display())));
    }
    read_video_info(path).map_err(|err| ("video_unreadable", format!("{err:#}")))
}

/// Each distinct video file once, in first-use order. A v3 file is shared by many episodes.
fn unique_video_paths(ds: &Dataset) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    ds.episodes
        .iter()
        .flat_map(|e| &e.videos)
        .filter(|v| seen.insert(&v.path))
        .map(|v| v.path.clone())
        .collect()
}

fn metadata_and_video_issues(ds: &Dataset, reads: &HashMap<PathBuf, VideoRead>) -> Vec<Tagged> {
    use Severity::*;
    let mut out = Vec::new();
    let mut prev_to: Option<i64> = None;
    let mut latest: HashMap<&str, &Path> = HashMap::new();
    for (pos, e) in ds.episodes.iter().enumerate() {
        let ep = Some(e.index);
        let mut meta =
            |s, code, message: String| out.push(tag(Some(pos), PHASE_META, s, ep, code, message));
        if e.index != pos {
            meta(
                Error,
                "episode_index",
                format!("episode at position {pos} has index {}", e.index),
            );
        }
        if let Some((from, to)) = e.range {
            if to - from != e.length as i64 {
                meta(
                    Error,
                    "range_length",
                    format!(
                        "dataset range [{from}, {to}) has {} frames, length says {}",
                        to - from,
                        e.length
                    ),
                );
            }
            if let Some(p) = prev_to
                && p != from
            {
                meta(
                    Error,
                    "range_gap",
                    format!("starts at {from}, previous episode ended at {p}"),
                );
            }
            prev_to = Some(to);
        }
        for v in &e.videos {
            // Report per-file findings once per file, not once per episode that shares it.
            let fresh = latest.insert(&v.key, &v.path) != Some(v.path.as_path());
            let mut vid = |s, code, message: String| {
                out.push(tag(Some(pos), PHASE_VIDEO, s, ep, code, message))
            };
            match &reads[&v.path] {
                Err((code, msg)) => {
                    if fresh {
                        vid(Error, code, msg.clone());
                    }
                }
                Ok(info) => {
                    let frame = 1.0 / ds.info.fps;
                    if fresh && (info.fps() / ds.info.fps - 1.0).abs() > VIDEO_FPS_TOL {
                        vid(
                            Warning,
                            "video_fps",
                            format!(
                                "{} runs at {:.3} fps, info.json says {}",
                                v.key,
                                info.fps(),
                                ds.info.fps
                            ),
                        );
                    }
                    match v.span {
                        None if info.sample_count != e.length as u64 => vid(
                            Error,
                            "video_frame_count",
                            format!(
                                "{} has {} frames, episode has {}",
                                v.key, info.sample_count, e.length
                            ),
                        ),
                        Some((from, to)) => {
                            let expected = e.length as f64 * frame;
                            let span_ok = ((to - from) - expected).abs() <= frame
                                && from >= -frame
                                && to <= info.duration_s() + frame;
                            if ((to - from) - expected).abs() > frame {
                                vid(
                                    Error,
                                    "video_span_length",
                                    format!(
                                        "{}: span [{from:.3}, {to:.3}) s is {:.3} s, {} frames are {expected:.3} s",
                                        v.key,
                                        to - from,
                                        e.length
                                    ),
                                );
                            }
                            if from < -frame || to > info.duration_s() + frame {
                                vid(
                                    Error,
                                    "video_span_beyond_end",
                                    format!(
                                        "{}: span [{from:.3}, {to:.3}) s outside video of {:.3} s",
                                        v.key,
                                        info.duration_s()
                                    ),
                                );
                            }
                            // A span with the right length can still hold the wrong frames (dropped or duplicated).
                            let n = info.samples_in(from, to);
                            if span_ok && n != e.length as u64 {
                                vid(
                                    Error,
                                    "video_frame_count",
                                    format!(
                                        "{}: span [{from:.3}, {to:.3}) s holds {n} frames, episode has {}",
                                        v.key, e.length
                                    ),
                                );
                            }
                        }
                        None => {}
                    }
                }
            }
        }
    }
    out
}

/// Load one data file and check every episode stored in it. `eps` all share `eps[0].data_file`.
fn check_group(ds: &Dataset, action: Option<&str>, start: usize, eps: &[Episode]) -> Vec<Tagged> {
    use Severity::Error;
    let first = &eps[0];
    let at_first = |code, message: String| {
        vec![tag(
            Some(start),
            PHASE_DATA,
            Error,
            Some(first.index),
            code,
            message,
        )]
    };
    // One read syscall for the whole file: parquet's own reader issues many small preads, which stopped
    // scaling past ~3 threads (synthetic_v2 jobs10: 98 ms with File, 42 ms with this).
    let Ok(bytes) = std::fs::read(&first.data_file) else {
        return at_first("data_missing", format!("{}", first.data_file.display()));
    };
    let d = match parse_file(bytes.into(), ds) {
        Ok(d) if d.ts.len() != d.rows => {
            return at_first(
                "column_missing",
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
    eps.iter()
        .enumerate()
        .flat_map(|(i, e)| {
            check_episode(ds, action, e, &d, &runs)
                .into_iter()
                .map(move |x| (Some(start + i), PHASE_DATA, x))
        })
        .collect()
}

fn check_episode(
    ds: &Dataset,
    action: Option<&str>,
    e: &Episode,
    d: &FileData,
    runs: &HashMap<i64, Run>,
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

    // This episode's rows: its run in a packed file, or the whole file if there is no episode_index column.
    let (s, t) = match &d.ep {
        Some(_) => match runs.get(&(e.index as i64)) {
            Some(r) => *r,
            None => {
                push(
                    Error,
                    "episode_rows_missing",
                    "no rows with this episode_index in its data file".into(),
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
            format!("metadata says {} frames, data file has {}", e.length, t - s),
        );
    }
    if let (Some((from, _)), Some(idx)) = (e.range, &d.idx)
        && idx[s..t]
            .iter()
            .enumerate()
            .any(|(i, x)| *x != from + i as i64)
    {
        push(
            Error,
            "index_mismatch",
            format!("global `index` column does not run from {from}"),
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
        }
    }
    v
}

fn as_f64(c: &ArrayRef) -> Vec<f64> {
    if let Some(a) = c.as_primitive_opt::<Float32Type>() {
        return a.values().iter().map(|x| *x as f64).collect();
    }
    if let Some(a) = c.as_primitive_opt::<Float64Type>() {
        return a.values().to_vec();
    }
    Vec::new()
}

/// Flatten a (fixed-size) list column of floats into (values, row width).
fn flatten(c: &ArrayRef) -> Option<(Vec<f64>, usize)> {
    if let Some(l) = c.as_fixed_size_list_opt() {
        return Some((as_f64(l.values()), l.value_length() as usize));
    }
    let l = c.as_list_opt::<i32>()?;
    let w = if l.is_empty() {
        0
    } else {
        l.value_length(0) as usize
    };
    Some((as_f64(l.values()), w))
}

#[cfg(test)]
mod tests {
    use super::*;

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
                last_move: Some(5)
            }
        );
    }

    #[test]
    fn idle_profile_handles_degenerate_input() {
        assert_eq!(idle_profile(&[], 3), Idle::default());
        assert_eq!(idle_profile(&[1.0, 2.0], 0), Idle::default()); // zero width must not panic
        assert_eq!(idle_profile(&[1.0, 2.0], 2).transitions, 0); // a single row has no transitions
    }
}
