use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

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
fn default_chunks_size() -> usize { 1000 }

#[derive(Debug, Deserialize)]
pub struct EpisodeMeta {
    pub episode_index: usize,
    pub length: usize,
}

pub struct Dataset {
    pub root: PathBuf,
    pub info: Info,
    pub episodes: Vec<EpisodeMeta>,
}

impl Dataset {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref().to_path_buf();
        let p = root.join("meta/info.json");
        let info: Info = serde_json::from_slice(&fs::read(&p).with_context(|| format!("reading {}", p.display()))?)
            .with_context(|| format!("parsing {}", p.display()))?;
        if !info.codebase_version.starts_with("v2") {
            bail!("unsupported codebase_version {:?} (only v2.x so far; v3 is on the roadmap)", info.codebase_version);
        }
        let p = root.join("meta/episodes.jsonl");
        let episodes = fs::read_to_string(&p)
            .with_context(|| format!("reading {}", p.display()))?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).with_context(|| format!("bad line in episodes.jsonl: {l}")))
            .collect::<Result<Vec<EpisodeMeta>>>()?;
        Ok(Self { root, info, episodes })
    }

    pub fn video_keys(&self) -> Vec<&str> {
        self.info.features.iter().filter(|(_, f)| f.dtype == "video").map(|(k, _)| k.as_str()).collect()
    }

    pub fn data_file(&self, ep: usize) -> PathBuf {
        self.root.join(fill(&self.info.data_path, ep / self.info.chunks_size, ep, ""))
    }

    pub fn video_file(&self, ep: usize, key: &str) -> Option<PathBuf> {
        let t = self.info.video_path.as_ref()?;
        Some(self.root.join(fill(t, ep / self.info.chunks_size, ep, key)))
    }
}

/// Fill Python-style `{name}` / `{name:03d}` placeholders used by LeRobot path templates.
pub fn fill(template: &str, chunk: usize, ep: usize, video_key: &str) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let Some(j) = rest[i..].find('}') else { break };
        let spec = &rest[i + 1..i + j];
        let (name, fmt) = spec.split_once(':').unwrap_or((spec, ""));
        let width: usize = fmt.trim_start_matches('0').trim_end_matches('d').parse().unwrap_or(0);
        let val = match name {
            "episode_chunk" => format!("{chunk:0width$}"),
            "episode_index" => format!("{ep:0width$}"),
            "video_key" => video_key.to_string(),
            other => format!("{{{other}}}"),
        };
        out.push_str(&val);
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
            fill("data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet", 0, 12, ""),
            "data/chunk-000/episode_000012.parquet"
        );
        assert_eq!(
            fill("videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4", 1, 1042, "observation.images.top"),
            "videos/chunk-001/observation.images.top/episode_001042.mp4"
        );
    }
}
