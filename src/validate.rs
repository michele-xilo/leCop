use crate::dataset::Dataset;
use anyhow::Result;
use arrow_array::{Array, ArrayRef, cast::AsArray, types::*};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use std::fs::File;

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

pub fn validate(ds: &Dataset) -> Result<Vec<Issue>> {
    let mut v = Vec::new();
    let mut push = |s, ep, code, message: String| v.push(Issue { severity: s, episode: ep, code, message });
    use Severity::*;

    if ds.episodes.len() != ds.info.total_episodes {
        push(Error, None, "episode_count", format!("info.json says {} episodes, episodes.jsonl has {}", ds.info.total_episodes, ds.episodes.len()));
    }
    let sum: usize = ds.episodes.iter().map(|e| e.length).sum();
    if sum != ds.info.total_frames {
        push(Error, None, "frame_count", format!("info.json says {} frames, episodes.jsonl sums to {sum}", ds.info.total_frames));
    }

    for (pos, e) in ds.episodes.iter().enumerate() {
        let ep = Some(e.episode_index);
        if e.episode_index != pos {
            push(Error, ep, "episode_index", format!("episode at position {pos} has index {}", e.episode_index));
        }
        for key in ds.video_keys() {
            if let Some(p) = ds.video_file(e.episode_index, key) {
                if !p.exists() { push(Error, ep, "video_missing", format!("{}", p.display())); }
            }
        }
        let path = ds.data_file(e.episode_index);
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => { push(Error, ep, "data_missing", format!("{}", path.display())); continue; }
        };
        let reader = match ParquetRecordBatchReaderBuilder::try_new(file).and_then(|b| b.build()) {
            Ok(r) => r,
            Err(err) => { push(Error, ep, "data_unreadable", err.to_string()); continue; }
        };

        let (mut ts, mut fi) = (Vec::<f64>::new(), Vec::<i64>::new());
        let mut nonfinite = std::collections::BTreeMap::<String, usize>::new();
        let mut action: Vec<f64> = Vec::new();
        let mut action_width = 0;
        for batch in reader {
            let batch = batch?;
            if let Some(c) = batch.column_by_name("timestamp") { ts.extend(as_f64(c)); }
            if let Some(c) = batch.column_by_name("frame_index") {
                if let Some(a) = c.as_primitive_opt::<Int64Type>() { fi.extend(a.values().iter().copied()); }
            }
            for (key, f) in &ds.info.features {
                if f.dtype != "float32" && f.dtype != "float64" { continue; }
                let Some(c) = batch.column_by_name(key) else { continue };
                let Some((vals, w)) = flatten(c) else { continue };
                *nonfinite.entry(key.clone()).or_default() += vals.iter().filter(|x| !x.is_finite()).count();
                if key == "action" { action.extend(vals); action_width = w; }
            }
        }

        if ts.len() != e.length {
            push(Error, ep, "length_mismatch", format!("episodes.jsonl says {} frames, parquet has {}", e.length, ts.len()));
        }
        if let Some(i) = ts.windows(2).position(|w| w[1] <= w[0]) {
            push(Error, ep, "timestamp_not_monotonic", format!("at frame {} ({} -> {})", i + 1, ts[i], ts[i + 1]));
        }
        if let Some((i, t)) = ts.iter().enumerate().find(|(i, t)| (**t - *i as f64 / ds.info.fps).abs() > TIMESTAMP_TOL_S) {
            push(Error, ep, "timestamp_drift", format!("frame {i}: t={t:.5}s, expected {:.5}s at {} fps", i as f64 / ds.info.fps, ds.info.fps));
        }
        if !fi.is_empty() && fi.iter().enumerate().any(|(i, f)| *f != i as i64) {
            push(Error, ep, "frame_index", "frame_index is not 0..n-1".into());
        }
        for (k, n) in nonfinite.into_iter().filter(|(_, n)| *n > 0) {
            push(Error, ep, "non_finite", format!("{k}: {n} NaN/Inf values"));
        }
        if action_width > 0 && action.len() > action_width && action.chunks(action_width).all(|r| r == &action[..action_width]) {
            push(Warning, ep, "action_constant", "action is identical on every frame".into());
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
