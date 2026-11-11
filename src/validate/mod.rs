mod episodes;
mod jump_shift;
mod limits;
mod numeric;
mod portability;
mod schema;
mod stats;
mod tasks;
mod video;
pub use episodes::{Idle, idle_profile, length_findings, rows_hash};
pub use jump_shift::{
    Moments, StepMoments, StepScale, count_jumps, moments, shift_finding, step_moments,
};
pub use limits::{Bound, FeatureLimits, Limits};
pub use numeric::{DimStats, dim_stats, numeric_findings};
pub use tasks::{task_count, task_finding};

use self::episodes::{
    ActionHash, GroupExtra, action_key, check_group, chunk_finding, duplicate_episodes,
};
use self::jump_shift::{jump_issues, shift_issues};
use self::limits::limit_issues;
use self::numeric::{DEFAULT_CLIPPED_FRACTION, merge_stats, numeric_issues};
use self::portability::portability_findings;
use self::stats::{stats_findings, stats_mismatch_findings, zero_std_findings};
use self::tasks::{task_ids, task_index_issue};
use self::video::{
    VideoRead, decode_issues, metadata_and_video_issues, read_video, unique_video_paths,
};
use crate::dataset::Dataset;
use crate::dataset::Episode;
use anyhow::Context;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;

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

/// What to check beyond the defaults.
#[derive(Debug, Clone, Default)]
pub struct Checks {
    pub limits: Limits,
    /// Decode the video frames (needs the `decode` cargo feature).
    pub decode: bool,
    /// Frames a training window spans (a policy's chunk size); episodes shorter than this are reported.
    pub chunk_size: Option<usize>,
    /// Fraction of a dimension's values at its minimum or maximum from which it is reported as clipped; 0.05 if unset.
    pub clipped_fraction: Option<f64>,
}

/// Every issue code lecop can report.
pub const ALL_CODES: [&str; 57] = [
    "episode_count",
    "action_jump",
    "distribution_shift",
    "task_index",
    "stats_missing",
    "stats_feature_missing",
    "tasks_missing",
    "tasks_count",
    "frame_count",
    "episode_index",
    "range_length",
    "range_gap",
    "index_mismatch",
    "data_missing",
    "data_unreadable",
    "length_mismatch",
    "episode_rows_missing",
    "timestamp_drift",
    "timestamp_not_monotonic",
    "frame_index",
    "timestamp_missing",
    "column_shape",
    "feature_missing",
    "feature_dtype",
    "feature_shape",
    "feature_extra",
    "episode_empty",
    "episode_short",
    "episode_length_outlier",
    "duplicate_episode",
    "constant_dimension",
    "clipped_dimension",
    "outlier_values",
    "video_missing",
    "video_unreadable",
    "video_fps",
    "video_resolution",
    "video_frame_count",
    "video_span_length",
    "video_span_beyond_end",
    "nonstandard_features",
    "non_finite",
    "action_constant",
    "idle_episode",
    "path_absolute",
    "data_stray_file",
    "limit_out_of_range",
    "limit_step",
    "limit_width",
    "video_decode",
    "video_frozen",
    "video_black",
    "symlink",
    "file_oversized",
    "stats_zero_std",
    "episodes_below_chunk",
    "stats_mismatch",
];

/// One sentence saying what an issue code means.
pub fn explain(code: &str) -> &'static str {
    match code {
        "episode_count" => "info.json total_episodes disagrees with the episode metadata",
        "action_jump" => "the action changes by far more than usual between consecutive frames",
        "distribution_shift" => {
            "the first and last quarter of the episodes have clearly different mean values"
        }
        "task_index" => "a task_index in the data is not in the task metadata",
        "stats_missing" => "the statistics file for this format version is missing or unreadable",
        "stats_feature_missing" => {
            "a non-string feature declared in info.json has no entry in the statistics"
        }
        "tasks_missing" => {
            "info.json declares tasks but the task metadata file is missing or unreadable"
        }
        "tasks_count" => {
            "info.json total_tasks disagrees with the number of tasks in the task metadata"
        }
        "path_absolute" => {
            "data_path or video_path in info.json is an absolute path, so the dataset only works on this machine"
        }
        "limit_out_of_range" => {
            "a value is outside the min or max set for its feature in lecop.toml"
        }
        "limit_step" => "the action changes between two frames by more than max_step in lecop.toml",
        "symlink" => {
            "the dataset contains a symbolic link, which breaks when it is copied or uploaded"
        }
        "file_oversized" => "a file is over 5 GB, the size the Hub recommends staying under",
        "video_decode" => "video frames could not be decoded (only with --decode)",
        "video_frozen" => {
            "a camera repeats the same picture for a second or more (only with --decode)"
        }
        "video_black" => "a camera shows a black picture for a second or more (only with --decode)",
        "stats_zero_std" => {
            "stats.json gives a dimension zero std, so normalising it divides by zero"
        }
        "stats_mismatch" => {
            "stats.json has a count, min, max, mean or std that differs from the data, so it is out of date"
        }
        "episodes_below_chunk" => {
            "episodes are shorter than the chunk_size set under [training] in lecop.toml"
        }
        "limit_width" => "a min or max list in lecop.toml does not have one value per dimension",
        "data_stray_file" => "the data directory holds a file that is not a parquet file",
        "frame_count" => "info.json total_frames disagrees with the sum of the episode lengths",
        "episode_index" => "episode indices do not run 0..n-1 in order",
        "range_length" => "an episode's dataset index range is not as long as the episode",
        "range_gap" => "an episode does not start where the previous one ended",
        "index_mismatch" => "the global index column does not continue across the episode",
        "data_missing" => "a data parquet file listed by the metadata does not exist",
        "data_unreadable" => "a data parquet file is truncated, unfinalized or not parquet",
        "length_mismatch" => "an episode has a different number of rows than its metadata says",
        "episode_rows_missing" => "the data file has no rows for an episode its metadata lists",
        "timestamp_drift" => "a timestamp differs from frame_index / fps by more than 1e-4 s",
        "timestamp_not_monotonic" => "timestamps do not strictly increase within an episode",
        "frame_index" => "frame_index does not run 0..n-1 within the episode",
        "timestamp_missing" => "the timestamp column is missing or not a float",
        "column_shape" => "a feature column does not have a consistent row width",
        "feature_missing" => "a feature declared in info.json has no column in the data files",
        "feature_dtype" => "a data column has a different type than info.json declares",
        "feature_shape" => "a data column has a different width than info.json declares",
        "feature_extra" => "a data column is not declared in info.json",
        "episode_empty" => "the episode metadata says the episode has no frames",
        "episode_short" => "the episode is shorter than a second (or 5 frames)",
        "episode_length_outlier" => {
            "the episode length is more than 3 standard deviations from the mean"
        }
        "duplicate_episode" => "the episode has exactly the same action sequence as an earlier one",
        "constant_dimension" => {
            "a dimension of the action or an observation has the same value in every frame"
        }
        "clipped_dimension" => {
            "a dimension sits at its minimum or maximum in at least clipped_fraction of the frames (5% by default)"
        }
        "outlier_values" => {
            "a value is more than 10 standard deviations from the mean of its dimension"
        }
        "video_missing" => "a video file listed by the metadata does not exist",
        "video_unreadable" => "a video file is truncated or not a valid mp4",
        "video_fps" => "a video's frame rate differs from the fps in info.json",
        "video_resolution" => "a video's frame size differs from the shape in info.json",
        "video_frame_count" => "the video holds a different number of frames than the episode",
        "video_span_length" => "the episode's span in a shared video is not as long as the episode",
        "video_span_beyond_end" => "the episode's span reaches past the end of the video",
        "nonstandard_features" => "there is no action feature, which policies expect",
        "non_finite" => "a float feature contains NaN or Inf",
        "action_constant" => "the action is identical on every frame",
        "idle_episode" => "more than half of the frames repeat the previous action",
        _ => "",
    }
}

/// An issue plus where it sorts in the report: episode position (None = dataset-wide) and check phase.
pub(super) type Tagged = (Option<usize>, u8, Issue);

pub(super) const PHASE_META: u8 = 0;

pub(super) const PHASE_VIDEO: u8 = 1;

pub(super) const PHASE_DATA: u8 = 2;

pub(super) fn tag(
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
    validate_with_checks(ds, jobs, &Checks::default())
}

/// Like [`validate_with`], with the extra `checks`.
pub fn validate_with_checks(ds: &Dataset, jobs: usize, checks: &Checks) -> Result<Vec<Issue>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .context("building thread pool")?;
    Ok(pool.install(|| run(ds, checks)))
}

/// Validate several datasets on one shared pool of `jobs` threads (0 = one per core). Results come back in
/// input order, so the report does not depend on scheduling.
pub fn validate_paths(
    paths: &[PathBuf],
    jobs: usize,
    checks: &Checks,
) -> Result<Vec<(PathBuf, Result<Vec<Issue>>)>> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .build()
        .context("building thread pool")?;
    Ok(pool.install(|| {
        paths
            .par_iter()
            .map(|p| {
                let issues = Dataset::open(p.clone()).map(|ds| run(&ds, checks));
                match &issues {
                    Ok(i) => tracing::info!(dataset = %p.display(), findings = i.len(), "validated"),
                    Err(e) => tracing::warn!(dataset = %p.display(), error = %format!("{e:#}"), "cannot open"),
                }
                (p.clone(), issues)
            })
            .collect()
    }))
}

pub(super) fn run(ds: &Dataset, checks: &Checks) -> Vec<Issue> {
    // The file walk is mostly waiting on the filesystem: run it beside the checks instead of before them.
    let (port, mut tagged) = rayon::join(|| portability_findings(ds), || run_tagged(ds, checks));
    tagged.extend(
        port.into_iter()
            .map(|(code, m)| tag(None, PHASE_META, Severity::Warning, None, code, m)),
    );
    // Stable sort: ties keep their within-phase order, so the report does not depend on thread scheduling.
    tagged.sort_by_key(|(pos, phase, _)| (*pos, *phase));
    // A data file missing from several separate runs of episodes is reported once.
    let mut seen = std::collections::HashSet::new();
    tagged.retain(|(_, _, i)| i.code != "data_missing" || seen.insert(i.message.clone()));
    tagged.into_iter().map(|(_, _, i)| i).collect()
}

pub(super) fn run_tagged(ds: &Dataset, checks: &Checks) -> Vec<Tagged> {
    let action = action_key(ds);
    let mut tagged = global_issues(ds, action, checks.chunk_size);

    let reads: HashMap<PathBuf, VideoRead> = unique_video_paths(ds)
        .into_par_iter()
        .map(|p| {
            let r = read_video(&p);
            (p, r)
        })
        .collect();
    tagged.extend(metadata_and_video_issues(ds, &reads));
    if checks.decode {
        tagged.extend(decode_issues(ds, &reads));
    }

    // One unit of work per run of episodes sharing a data file: a v2 file is one episode, a v3 file many.
    let mut groups: Vec<(usize, &[Episode])> = Vec::new();
    let mut start = 0;
    for g in ds.episodes.chunk_by(|a, b| a.data_file == b.data_file) {
        groups.push((start, g));
        start += g.len();
    }
    let data: Vec<(Vec<Tagged>, GroupExtra)> = groups
        .into_par_iter()
        .map(|(start, eps)| check_group(ds, action, start, eps))
        .collect();
    let (data, extras): (Vec<_>, Vec<_>) = data.into_iter().unzip();
    let hashes: Vec<ActionHash> = extras
        .iter()
        .flat_map(|e| e.hashes.iter().copied())
        .collect();
    tagged.extend(duplicate_episodes(&hashes));
    let used: BTreeSet<i64> = extras
        .iter()
        .flat_map(|e| e.tasks.iter().copied())
        .collect();
    tagged.extend(task_index_issue(&used, task_ids(ds).as_ref()));
    tagged.extend(numeric_issues(
        &extras,
        action,
        checks.clipped_fraction.unwrap_or(DEFAULT_CLIPPED_FRACTION),
    ));
    for (code, message) in stats_mismatch_findings(ds, &merge_stats(&extras)) {
        tagged.push(tag(
            None,
            PHASE_DATA,
            Severity::Warning,
            None,
            code,
            message,
        ));
    }
    tagged.extend(limit_issues(ds, action, &extras, &checks.limits));
    tagged.extend(shift_issues(ds, &extras));
    tagged.extend(jump_issues(ds, action, &extras));
    // A schema fault is the same in every file of the dataset: report it once, at the first file.
    let mut seen = std::collections::HashSet::new();
    tagged.extend(data.into_iter().flatten().filter(|(_, _, i)| {
        !i.code.starts_with("feature_") || seen.insert((i.code, i.message.clone()))
    }));
    tagged
}

pub(super) fn global_issues(
    ds: &Dataset,
    action: Option<&str>,
    chunk: Option<usize>,
) -> Vec<Tagged> {
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
    if !ds.info.features.contains_key("observation.state") {
        v.push(tag(
            None,
            PHASE_META,
            Warning,
            None,
            "nonstandard_features",
            "no `observation.state` feature, which LeRobot policies expect".into(),
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
    for (code, message) in stats_findings(ds) {
        v.push(tag(
            None,
            PHASE_META,
            Severity::Warning,
            None,
            code,
            message,
        ));
    }
    if let Some((sev, code, message)) = task_finding(ds.info.total_tasks, task_count(ds)) {
        v.push(tag(None, PHASE_META, sev, None, code, message));
    }
    for (code, message) in zero_std_findings(ds) {
        v.push(tag(None, PHASE_META, Warning, None, code, message));
    }
    let lengths: Vec<usize> = ds.episodes.iter().map(|e| e.length).collect();
    if let Some(m) = chunk.and_then(|c| chunk_finding(&lengths, c)) {
        v.push(tag(
            None,
            PHASE_META,
            Warning,
            None,
            "episodes_below_chunk",
            m,
        ));
    }
    if !lengths.is_empty() {
        for (i, sev, code, message) in length_findings(&lengths, ds.info.fps) {
            v.push(tag(
                Some(i),
                PHASE_META,
                sev,
                Some(ds.episodes[i].index),
                code,
                message,
            ));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explanation_does_not_hard_code_the_clipped_threshold() {
        // The threshold is `clipped_fraction`, default 5% and configurable.
        assert!(!explain("clipped_dimension").contains("99%"));
    }

    #[test]
    fn an_unknown_code_has_no_explanation() {
        assert_eq!(explain("no_such_code"), "");
    }

    #[test]
    fn every_code_has_an_explanation() {
        for c in ALL_CODES {
            assert!(!explain(c).is_empty(), "{c}");
        }
    }
}
