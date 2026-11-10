use super::{PHASE_DATA, PHASE_META, PHASE_VIDEO, Severity, Tagged, tag};
use crate::dataset::Dataset;
use crate::mp4::VideoInfo;
use crate::mp4::read_video_info;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

/// Relative difference between a video's frame rate and info.json's that we still accept.
pub(super) const VIDEO_FPS_TOL: f64 = 0.01;

/// Outcome of reading one video file: its info, or (issue code, message).
pub(super) type VideoRead = Result<VideoInfo, (&'static str, String)>;

pub(super) fn read_video(path: &Path) -> VideoRead {
    if !path.exists() {
        tracing::debug!(file = %path.display(), "video file missing");
        return Err(("video_missing", format!("{}", path.display())));
    }
    let started = std::time::Instant::now();
    let r = read_video_info(path).map_err(|err| ("video_unreadable", format!("{err:#}")));
    tracing::debug!(
        file = %path.display(),
        ok = r.is_ok(),
        elapsed_ms = started.elapsed().as_secs_f64() * 1e3,
        "read video metadata"
    );
    r
}

/// Each distinct video file once, in first-use order. A v3 file is shared by many episodes.
pub(super) fn unique_video_paths(ds: &Dataset) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    ds.episodes
        .iter()
        .flat_map(|e| &e.videos)
        .filter(|v| seen.insert(&v.path))
        .map(|v| v.path.clone())
        .collect()
}

/// Decode every readable video once and report what the frames show. A video that cannot be decoded at all
/// (an unsupported codec) is reported too, so the output never implies it was checked.
pub(super) fn decode_issues(ds: &Dataset, reads: &HashMap<PathBuf, VideoRead>) -> Vec<Tagged> {
    // A v2 video holds one episode; a v3 file holds many, so its findings belong to the file.
    let owner: HashMap<&Path, usize> = ds
        .episodes
        .iter()
        .enumerate()
        .flat_map(|(pos, e)| {
            e.videos
                .iter()
                .filter(|v| v.span.is_none())
                .map(move |v| (v.path.as_path(), pos))
        })
        .collect();
    let mut paths: Vec<&PathBuf> = reads
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(p, _)| p)
        .collect();
    paths.sort();
    paths
        .into_par_iter()
        .flat_map(|path| {
            let found: Vec<(Severity, &'static str, String)> =
                match crate::decode::decode_checked(path) {
                    Ok(d) => crate::decode::video_findings(&d)
                        .into_iter()
                        .map(|(code, m)| {
                            let sev = if code == "video_decode" {
                                Severity::Error
                            } else {
                                Severity::Warning
                            };
                            (sev, code, m)
                        })
                        .collect(),
                    Err(e) => vec![(
                        Severity::Warning,
                        "video_decode",
                        format!("not decoded: {e:#}"),
                    )],
                };
            let pos = owner.get(path.as_path()).copied();
            let name = path
                .strip_prefix(&ds.root)
                .unwrap_or(path)
                .display()
                .to_string();
            found
                .into_iter()
                .map(|(sev, code, m)| {
                    tag(
                        pos,
                        PHASE_DATA,
                        sev,
                        pos.map(|p| ds.episodes[p].index),
                        code,
                        format!("{name}: {m}"),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn metadata_and_video_issues(
    ds: &Dataset,
    reads: &HashMap<PathBuf, VideoRead>,
) -> Vec<Tagged> {
    use Severity::*;
    let mut out = Vec::new();
    let mut prev_to: Option<i64> = None;
    let mut latest: HashMap<&str, &Path> = HashMap::new();
    // A wrong size is the same in every file of a camera: report it once per camera.
    let mut wrong_size: std::collections::HashSet<&str> = std::collections::HashSet::new();
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
                    if let (Some((w, h)), Some((dw, dh))) = (
                        info.size,
                        ds.info.features.get(&v.key).and_then(|f| f.frame_size()),
                    ) && (w as usize, h as usize) != (dw, dh)
                        && wrong_size.insert(v.key.as_str())
                    {
                        vid(
                            Error,
                            "video_resolution",
                            format!(
                                "{} is {w}x{h} (width x height) but info.json declares {dw}x{dh}",
                                v.key
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
