use anyhow::{Context, Result, bail};
use arrow_array::{RecordBatch, cast::AsArray, types::*};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
pub struct Feature {
    pub dtype: String,
    #[serde(default)]
    pub shape: Vec<usize>,
}

#[derive(Debug, Deserialize)]
pub struct Info {
    pub codebase_version: String,
    #[serde(default)]
    pub robot_type: Option<String>,
    pub fps: f64,
    pub total_episodes: usize,
    pub total_frames: usize,
    #[serde(default = "default_chunks_size")]
    pub chunks_size: usize,
    pub data_path: String,
    #[serde(default)]
    pub video_path: Option<String>,
    pub features: BTreeMap<String, Feature>,
}
fn default_chunks_size() -> usize {
    1000
}

/// One episode, resolved to files. Same shape for v2.x (one file per episode) and v3.0 (packed files).
#[derive(Debug)]
pub struct Episode {
    pub index: usize,
    pub length: usize,
    pub data_file: PathBuf,
    /// v3 only: the episode's global frame range `[from, to)` from the episodes metadata.
    pub range: Option<(i64, i64)>,
    pub videos: Vec<VideoRef>,
}

/// One camera's video for an episode.
#[derive(Debug)]
pub struct VideoRef {
    pub key: String,
    pub path: PathBuf,
    /// v3 only: where the episode sits inside the (shared) video file, in seconds `[from, to)`.
    pub span: Option<(f64, f64)>,
}

pub struct Dataset {
    pub root: PathBuf,
    pub info: Info,
    pub episodes: Vec<Episode>,
}

impl Dataset {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let p = root.join("meta/info.json");
        let info: Info = serde_json::from_slice(
            &fs::read(&p).with_context(|| format!("reading {}", p.display()))?,
        )
        .with_context(|| format!("parsing {}", p.display()))?;
        if !(info.fps.is_finite() && info.fps > 0.0) {
            bail!("info.json fps must be a positive number, got {}", info.fps);
        }
        let episodes = match info.codebase_version.as_str() {
            v if v.starts_with("v2") => episodes_v2(&root, &info)?,
            v if v.starts_with("v3") => episodes_v3(&root, &info)?,
            v => bail!("unsupported codebase_version {v:?} (supported: v2.x, v3.x)"),
        };
        tracing::debug!(
            root = %root.display(),
            version = %info.codebase_version,
            episodes = episodes.len(),
            "opened dataset"
        );
        Ok(Self {
            root,
            info,
            episodes,
        })
    }
}

fn video_keys(info: &Info) -> Vec<&str> {
    info.features
        .iter()
        .filter(|(_, f)| f.dtype == "video")
        .map(|(k, _)| k.as_str())
        .collect()
}

#[derive(Deserialize)]
struct EpisodeLine {
    episode_index: usize,
    length: usize,
}

fn episodes_v2(root: &Path, info: &Info) -> Result<Vec<Episode>> {
    let p = root.join("meta/episodes.jsonl");
    let lines = fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
    let mut out = Vec::new();
    for l in lines.lines().filter(|l| !l.trim().is_empty()) {
        let e: EpisodeLine =
            serde_json::from_str(l).with_context(|| format!("bad line in episodes.jsonl: {l}"))?;
        let chunk = (e.episode_index / info.chunks_size).to_string();
        let ep = e.episode_index.to_string();
        let data_file = root.join(fill(
            &info.data_path,
            &[("episode_chunk", &chunk), ("episode_index", &ep)],
        ));
        let videos = match &info.video_path {
            Some(t) => video_keys(info)
                .into_iter()
                .map(|k| VideoRef {
                    key: k.to_string(),
                    path: root.join(fill(
                        t,
                        &[
                            ("episode_chunk", &chunk),
                            ("episode_index", &ep),
                            ("video_key", k),
                        ],
                    )),
                    span: None,
                })
                .collect(),
            None => vec![],
        };
        out.push(Episode {
            index: e.episode_index,
            length: e.length,
            data_file,
            range: None,
            videos,
        });
    }
    Ok(out)
}

fn col_i64(b: &RecordBatch, name: &str) -> Option<Vec<i64>> {
    let c = b.column_by_name(name)?;
    if let Some(a) = c.as_primitive_opt::<Int64Type>() {
        return Some(a.values().to_vec());
    }
    c.as_primitive_opt::<Int32Type>()
        .map(|a| a.values().iter().map(|x| *x as i64).collect())
}

fn col_f64(b: &RecordBatch, name: &str) -> Option<Vec<f64>> {
    Some(
        b.column_by_name(name)?
            .as_primitive_opt::<Float64Type>()?
            .values()
            .to_vec(),
    )
}

/// One video key's columns in the episodes metadata.
struct VideoCols {
    key: String,
    chunk: Vec<i64>,
    file: Vec<i64>,
    span: Option<(Vec<f64>, Vec<f64>)>,
}

fn episodes_v3(root: &Path, info: &Info) -> Result<Vec<Episode>> {
    let dir = root.join("meta/episodes");
    let mut files = Vec::new();
    for chunk in fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let chunk = chunk?.path();
        if chunk.is_dir() {
            for f in fs::read_dir(&chunk)? {
                let f = f?.path();
                if f.extension().is_some_and(|x| x == "parquet") {
                    files.push(f);
                }
            }
        }
    }
    files.sort();
    let vkeys = video_keys(info);
    let mut out = Vec::new();
    for f in files {
        let reader = ParquetRecordBatchReaderBuilder::try_new(fs::File::open(&f)?)?.build()?;
        for b in reader {
            let b = b?;
            let need = |n: &str| {
                col_i64(&b, n).with_context(|| format!("{}: missing column {n}", f.display()))
            };
            let (idx, len) = (need("episode_index")?, need("length")?);
            let (from, to) = (need("dataset_from_index")?, need("dataset_to_index")?);
            let (dc, df) = (need("data/chunk_index")?, need("data/file_index")?);
            let vcols: Vec<VideoCols> = vkeys
                .iter()
                .filter_map(|k| {
                    Some(VideoCols {
                        key: k.to_string(),
                        chunk: col_i64(&b, &format!("videos/{k}/chunk_index"))?,
                        file: col_i64(&b, &format!("videos/{k}/file_index"))?,
                        span: col_f64(&b, &format!("videos/{k}/from_timestamp"))
                            .zip(col_f64(&b, &format!("videos/{k}/to_timestamp"))),
                    })
                })
                .collect();
            for i in 0..idx.len() {
                let data_file = root.join(fill(
                    &info.data_path,
                    &[
                        ("chunk_index", &dc[i].to_string()),
                        ("file_index", &df[i].to_string()),
                    ],
                ));
                let videos = match &info.video_path {
                    Some(t) => vcols
                        .iter()
                        .map(|c| VideoRef {
                            key: c.key.clone(),
                            path: root.join(fill(
                                t,
                                &[
                                    ("video_key", &c.key),
                                    ("chunk_index", &c.chunk[i].to_string()),
                                    ("file_index", &c.file[i].to_string()),
                                ],
                            )),
                            span: c.span.as_ref().map(|(f, t)| (f[i], t[i])),
                        })
                        .collect(),
                    None => vec![],
                };
                out.push(Episode {
                    index: idx[i] as usize,
                    length: len[i] as usize,
                    data_file,
                    range: Some((from[i], to[i])),
                    videos,
                });
            }
        }
    }
    out.sort_by_key(|e| e.index);
    Ok(out)
}

/// Fill Python-style `{name}` / `{name:03d}` placeholders used by LeRobot path templates.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else { break };
        let spec = &rest[i + 1..i + j];
        let (name, fmt) = spec.split_once(':').unwrap_or((spec, ""));
        let width: usize = fmt
            .trim_start_matches('0')
            .trim_end_matches('d')
            .parse()
            .unwrap_or(0);
        match vars.iter().find(|(k, _)| *k == name) {
            Some((_, v)) => out.push_str(&format!("{v:0>width$}")),
            None => out.push_str(&format!("{{{spec}}}")),
        }
        rest = &rest[i + j + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::fill;
    #[test]
    fn fills_lerobot_templates() {
        assert_eq!(
            fill(
                "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
                &[("episode_chunk", "0"), ("episode_index", "12")]
            ),
            "data/chunk-000/episode_000012.parquet"
        );
        assert_eq!(
            fill(
                "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
                &[
                    ("video_key", "observation.images.top"),
                    ("chunk_index", "1"),
                    ("file_index", "42")
                ]
            ),
            "videos/observation.images.top/chunk-001/file-042.mp4"
        );
    }
}
