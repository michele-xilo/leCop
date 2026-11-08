use crate::{
    dataset::Dataset,
    mp4::{VideoInfo, read_video_info},
};
use anyhow::Result;
use arrow_array::{Array, ArrayRef, cast::AsArray, types::*};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
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

/// The data file currently loaded: path, contents (None if unreadable) and per-episode runs.
type Loaded = (PathBuf, Option<FileData>, HashMap<i64, Run>);

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

fn read_file(path: &Path, ds: &Dataset) -> Result<FileData> {
    let reader = ParquetRecordBatchReaderBuilder::try_new(File::open(path)?)?.build()?;
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

pub fn validate(ds: &Dataset) -> Result<Vec<Issue>> {
    use Severity::*;
    let mut v = Vec::new();
    let mut push = |s, ep, code, message: String| {
        v.push(Issue {
            severity: s,
            episode: ep,
            code,
            message,
        })
    };

    let action = action_key(ds);
    if !ds.info.features.contains_key("action") {
        let found = action.map_or(String::new(), |k| {
            format!("; running the action checks on `{k}`")
        });
        push(
            Warning,
            None,
            "nonstandard_features",
            format!("no `action` feature, which LeRobot policies expect{found}"),
        );
    }

    if ds.episodes.len() != ds.info.total_episodes {
        push(
            Error,
            None,
            "episode_count",
            format!(
                "info.json says {} episodes, metadata has {}",
                ds.info.total_episodes,
                ds.episodes.len()
            ),
        );
    }
    let sum: usize = ds.episodes.iter().map(|e| e.length).sum();
    if sum != ds.info.total_frames {
        push(
            Error,
            None,
            "frame_count",
            format!(
                "info.json says {} frames, episode lengths sum to {sum}",
                ds.info.total_frames
            ),
        );
    }

    let mut missing_reported: std::collections::HashSet<PathBuf> = Default::default();
    let mut current: Option<Loaded> = None;
    let mut prev_to: Option<i64> = None;
    let mut videos: HashMap<String, (PathBuf, VideoRead)> = HashMap::new();

    for (pos, e) in ds.episodes.iter().enumerate() {
        let ep = Some(e.index);
        if e.index != pos {
            push(
                Error,
                ep,
                "episode_index",
                format!("episode at position {pos} has index {}", e.index),
            );
        }
        if let Some((from, to)) = e.range {
            if to - from != e.length as i64 {
                push(
                    Error,
                    ep,
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
                push(
                    Error,
                    ep,
                    "range_gap",
                    format!("starts at {from}, previous episode ended at {p}"),
                );
            }
            prev_to = Some(to);
        }
        for v in &e.videos {
            // A v3 video file is shared by many episodes, so read it once, and keep only the latest per camera.
            let fresh = videos.get(&v.key).is_none_or(|(p, _)| p != &v.path);
            if fresh {
                let r: VideoRead = if !v.path.exists() {
                    Err(("video_missing", format!("{}", v.path.display())))
                } else {
                    read_video_info(&v.path).map_err(|err| ("video_unreadable", format!("{err:#}")))
                };
                videos.insert(v.key.clone(), (v.path.clone(), r));
            }
            match &videos[&v.key].1 {
                Err((code, msg)) => {
                    if fresh {
                        push(Error, ep, code, msg.clone());
                    }
                }
                Ok(info) => {
                    let frame = 1.0 / ds.info.fps;
                    if fresh && (info.fps() / ds.info.fps - 1.0).abs() > VIDEO_FPS_TOL {
                        push(
                            Warning,
                            ep,
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
                        None if info.sample_count != e.length as u64 => push(
                            Error,
                            ep,
                            "video_frame_count",
                            format!(
                                "{} has {} frames, episode has {}",
                                v.key, info.sample_count, e.length
                            ),
                        ),
                        Some((from, to)) => {
                            let expected = e.length as f64 * frame;
                            if ((to - from) - expected).abs() > frame {
                                push(
                                    Error,
                                    ep,
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
                                push(
                                    Error,
                                    ep,
                                    "video_span_beyond_end",
                                    format!(
                                        "{}: span [{from:.3}, {to:.3}) s outside video of {:.3} s",
                                        v.key,
                                        info.duration_s()
                                    ),
                                );
                            }
                        }
                        None => {}
                    }
                }
            }
        }

        if current.as_ref().is_none_or(|c| c.0 != e.data_file) {
            let loaded = match File::open(&e.data_file) {
                Err(_) => {
                    if missing_reported.insert(e.data_file.clone()) {
                        push(
                            Error,
                            ep,
                            "data_missing",
                            format!("{}", e.data_file.display()),
                        );
                    }
                    None
                }
                Ok(_) => match read_file(&e.data_file, ds) {
                    Ok(d) if d.ts.len() != d.rows => {
                        push(
                            Error,
                            ep,
                            "column_missing",
                            format!(
                                "{}: `timestamp` column missing or not a float",
                                e.data_file.display()
                            ),
                        );
                        None
                    }
                    Ok(d) => Some(d),
                    Err(err) => {
                        push(Error, ep, "data_unreadable", format!("{err:#}"));
                        None
                    }
                },
            };
            let runs = loaded.as_ref().map(episode_runs).unwrap_or_default();
            current = Some((e.data_file.clone(), loaded, runs));
        }
        let Some((_, Some(d), runs)) = &current else {
            continue;
        };

        // This episode's rows: its run in a packed file, or the whole file if there is no episode_index column.
        let (s, t) = match &d.ep {
            Some(_) => match runs.get(&(e.index as i64)) {
                Some(r) => *r,
                None => {
                    push(
                        Error,
                        ep,
                        "episode_rows_missing",
                        "no rows with this episode_index in its data file".into(),
                    );
                    continue;
                }
            },
            None => (0, d.ts.len()),
        };
        if t - s != e.length {
            push(
                Error,
                ep,
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
                ep,
                "index_mismatch",
                format!("global `index` column does not run from {from}"),
            );
        }
        let ts = &d.ts[s..t];
        if let Some(i) = ts.windows(2).position(|w| w[1] <= w[0]) {
            push(
                Error,
                ep,
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
                ep,
                "timestamp_drift",
                format!(
                    "frame {i}: t={x:.5}s, expected {:.5}s at {} fps",
                    i as f64 / ds.info.fps,
                    ds.info.fps
                ),
            );
        }
        if !d.fi.is_empty() && d.fi[s..t].iter().enumerate().any(|(i, f)| *f != i as i64) {
            push(Error, ep, "frame_index", "frame_index is not 0..n-1".into());
        }
        for (k, (vals, w)) in &d.floats {
            let Some(slice) = vals.get(s * w..t * w) else {
                push(
                    Error,
                    ep,
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
                push(Error, ep, "non_finite", format!("{k}: {n} NaN/Inf values"));
            }
            if Some(k.as_str()) == action && *w > 0 && slice.len() > *w {
                let idle = idle_profile(slice, *w);
                if idle.idle == idle.transitions {
                    push(
                        Warning,
                        ep,
                        "action_constant",
                        "action is identical on every frame".into(),
                    );
                } else if idle.idle as f64 > IDLE_FRACTION * idle.transitions as f64 {
                    push(
                        Warning,
                        ep,
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
    }
    Ok(v)
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
