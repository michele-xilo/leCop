use crate::dataset::Dataset;
use anyhow::Result;
use arrow_array::{Array, ArrayRef, cast::AsArray, types::*};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::{collections::{BTreeMap, HashMap}, fs::File, path::{Path, PathBuf}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity { Error, Warning }

#[derive(Debug, Clone)]
pub struct Issue {
    pub severity: Severity,
    pub episode: Option<usize>,
    pub code: &'static str,
    pub message: String,
}

/// Timestamps may deviate from frame_index/fps by this much (LeRobot's default tolerance).
const TIMESTAMP_TOL_S: f64 = 1e-4;

/// One data parquet file in memory. v3 files hold many episodes.
// ponytail: whole file in memory, one file at a time. Fine for ~100 MB shards; stream by row group if files get huge.
#[derive(Default)]
struct FileData {
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
        if let Some(c) = batch.column_by_name("timestamp") { d.ts.extend(as_f64(c)); }
        if let Some(c) = batch.column_by_name("frame_index").and_then(|c| c.as_primitive_opt::<Int64Type>()) {
            d.fi.extend(c.values().iter().copied());
        }
        if let Some(c) = batch.column_by_name("episode_index").and_then(|c| c.as_primitive_opt::<Int64Type>()) {
            d.ep.get_or_insert_with(Vec::new).extend(c.values().iter().copied());
        }
        if let Some(c) = batch.column_by_name("index").and_then(|c| c.as_primitive_opt::<Int64Type>()) {
            d.idx.get_or_insert_with(Vec::new).extend(c.values().iter().copied());
        }
        for (key, f) in &ds.info.features {
            if f.dtype != "float32" && f.dtype != "float64" { continue; }
            let Some((vals, w)) = batch.column_by_name(key).and_then(flatten) else { continue };
            let e = d.floats.entry(key.clone()).or_insert((Vec::new(), w));
            e.0.extend(vals);
        }
    }
    Ok(d)
}

/// Row ranges per episode_index inside a file (episodes are stored contiguously).
fn episode_runs(d: &FileData) -> HashMap<i64, (usize, usize)> {
    let mut runs = HashMap::new();
    let Some(ep) = &d.ep else { return runs };
    for (i, e) in ep.iter().enumerate() {
        runs.entry(*e).and_modify(|r: &mut (usize, usize)| r.1 = i + 1).or_insert((i, i + 1));
    }
    runs
}

pub fn validate(ds: &Dataset) -> Result<Vec<Issue>> {
    use Severity::*;
    let mut v = Vec::new();
    let mut push = |s, ep, code, message: String| v.push(Issue { severity: s, episode: ep, code, message });

    if ds.episodes.len() != ds.info.total_episodes {
        push(Error, None, "episode_count", format!("info.json says {} episodes, metadata has {}", ds.info.total_episodes, ds.episodes.len()));
    }
    let sum: usize = ds.episodes.iter().map(|e| e.length).sum();
    if sum != ds.info.total_frames {
        push(Error, None, "frame_count", format!("info.json says {} frames, episode lengths sum to {sum}", ds.info.total_frames));
    }

    let mut missing_reported: std::collections::HashSet<PathBuf> = Default::default();
    let mut current: Option<(PathBuf, Option<FileData>, HashMap<i64, (usize, usize)>)> = None;
    let mut prev_to: Option<i64> = None;

    for (pos, e) in ds.episodes.iter().enumerate() {
        let ep = Some(e.index);
        if e.index != pos {
            push(Error, ep, "episode_index", format!("episode at position {pos} has index {}", e.index));
        }
        if let Some((from, to)) = e.range {
            if to - from != e.length as i64 {
                push(Error, ep, "range_length", format!("dataset range [{from}, {to}) has {} frames, length says {}", to - from, e.length));
            }
            if prev_to.is_some_and(|p| p != from) {
                push(Error, ep, "range_gap", format!("starts at {from}, previous episode ended at {}", prev_to.unwrap()));
            }
            prev_to = Some(to);
        }
        for (_, vp) in &e.videos {
            if !vp.exists() && missing_reported.insert(vp.clone()) {
                push(Error, ep, "video_missing", format!("{}", vp.display()));
            }
        }

        if current.as_ref().is_none_or(|c| c.0 != e.data_file) {
            let loaded = match File::open(&e.data_file) {
                Err(_) => {
                    if missing_reported.insert(e.data_file.clone()) {
                        push(Error, ep, "data_missing", format!("{}", e.data_file.display()));
                    }
                    None
                }
                Ok(_) => match read_file(&e.data_file, ds) {
                    Ok(d) => Some(d),
                    Err(err) => { push(Error, ep, "data_unreadable", err.to_string()); None }
                },
            };
            let runs = loaded.as_ref().map(episode_runs).unwrap_or_default();
            current = Some((e.data_file.clone(), loaded, runs));
        }
        let (_, Some(d), runs) = current.as_ref().unwrap() else { continue };

        // This episode's rows: its run in a packed file, or the whole file if there is no episode_index column.
        let (s, t) = match &d.ep {
            Some(_) => match runs.get(&(e.index as i64)) {
                Some(r) => *r,
                None => { push(Error, ep, "episode_rows_missing", "no rows with this episode_index in its data file".into()); continue; }
            },
            None => (0, d.ts.len()),
        };
        if t - s != e.length {
            push(Error, ep, "length_mismatch", format!("metadata says {} frames, data file has {}", e.length, t - s));
        }
        if let (Some((from, _)), Some(idx)) = (e.range, &d.idx) {
            if idx[s..t].iter().enumerate().any(|(i, x)| *x != from + i as i64) {
                push(Error, ep, "index_mismatch", format!("global `index` column does not run from {from}"));
            }
        }
        let ts = &d.ts[s..t];
        if let Some(i) = ts.windows(2).position(|w| w[1] <= w[0]) {
            push(Error, ep, "timestamp_not_monotonic", format!("at frame {} ({} -> {})", i + 1, ts[i], ts[i + 1]));
        }
        if let Some((i, x)) = ts.iter().enumerate().find(|(i, x)| (**x - *i as f64 / ds.info.fps).abs() > TIMESTAMP_TOL_S) {
            push(Error, ep, "timestamp_drift", format!("frame {i}: t={x:.5}s, expected {:.5}s at {} fps", i as f64 / ds.info.fps, ds.info.fps));
        }
        if !d.fi.is_empty() && d.fi[s..t].iter().enumerate().any(|(i, f)| *f != i as i64) {
            push(Error, ep, "frame_index", "frame_index is not 0..n-1".into());
        }
        for (k, (vals, w)) in &d.floats {
            let slice = &vals[s * w..t * w];
            let n = slice.iter().filter(|x| !x.is_finite()).count();
            if n > 0 { push(Error, ep, "non_finite", format!("{k}: {n} NaN/Inf values")); }
            if k == "action" && *w > 0 && slice.len() > *w && slice.chunks(*w).all(|r| r == &slice[..*w]) {
                push(Warning, ep, "action_constant", "action is identical on every frame".into());
            }
        }
    }
    Ok(v)
}

fn as_f64(c: &ArrayRef) -> Vec<f64> {
    if let Some(a) = c.as_primitive_opt::<Float32Type>() { return a.values().iter().map(|x| *x as f64).collect(); }
    if let Some(a) = c.as_primitive_opt::<Float64Type>() { return a.values().to_vec(); }
    Vec::new()
}

/// Flatten a (fixed-size) list column of floats into (values, row width).
fn flatten(c: &ArrayRef) -> Option<(Vec<f64>, usize)> {
    if let Some(l) = c.as_fixed_size_list_opt() {
        return Some((as_f64(l.values()), l.value_length() as usize));
    }
    let l = c.as_list_opt::<i32>()?;
    let w = if l.is_empty() { 0 } else { l.value_length(0) as usize };
    Some((as_f64(l.values()), w))
}
