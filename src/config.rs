//! `lecop.toml`: which issue codes to ignore or select, severity overrides, per-dataset ignores.
use crate::validate::{ALL_CODES, Issue, Severity};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PerDataset {
    #[serde(default)]
    ignore: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    ignore: Vec<String>,
    select: Option<Vec<String>>,
    /// Codes reported as errors even when they are warnings by default.
    #[serde(default)]
    error: Vec<String>,
    /// Codes reported as warnings even when they are errors by default.
    #[serde(default)]
    warn: Vec<String>,
    #[serde(default, rename = "per-dataset")]
    per_dataset: BTreeMap<String, PerDataset>,
    #[serde(skip)]
    dir: PathBuf,
}

fn check_codes(codes: &[String], what: &str) -> Result<()> {
    for c in codes {
        if !ALL_CODES.contains(&c.as_str()) {
            bail!("unknown issue code `{c}` in {what}");
        }
    }
    Ok(())
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut c: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        let what = path.display().to_string();
        check_codes(&c.ignore, &what)?;
        check_codes(c.select.as_deref().unwrap_or_default(), &what)?;
        check_codes(&c.error, &what)?;
        check_codes(&c.warn, &what)?;
        for p in c.per_dataset.values() {
            check_codes(&p.ignore, &what)?;
        }
        c.dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        Ok(c)
    }

    /// The nearest `lecop.toml` in `start` or one of its parents.
    pub fn find(start: &Path) -> Option<PathBuf> {
        let start = start.canonicalize().ok()?;
        let mut dir = if start.is_dir() {
            start
        } else {
            start.parent()?.to_path_buf()
        };
        loop {
            let p = dir.join("lecop.toml");
            if p.is_file() {
                return Some(p);
            }
            if !dir.pop() {
                return None;
            }
        }
    }

    /// Add `--ignore` and `--select` from the command line.
    pub fn add_cli(&mut self, ignore: &[String], select: &[String]) -> Result<()> {
        check_codes(ignore, "--ignore")?;
        check_codes(select, "--select")?;
        self.ignore.extend_from_slice(ignore);
        if !select.is_empty() {
            self.select
                .get_or_insert_with(Vec::new)
                .extend_from_slice(select);
        }
        Ok(())
    }

    fn dataset_ignores(&self, dataset: &Path) -> BTreeSet<&str> {
        let want = dataset
            .canonicalize()
            .unwrap_or_else(|_| dataset.to_path_buf());
        let mut out = BTreeSet::new();
        for (key, p) in &self.per_dataset {
            let joined = self.dir.join(key);
            let have = joined.canonicalize().unwrap_or(joined);
            if have == want {
                out.extend(p.ignore.iter().map(String::as_str));
            }
        }
        out
    }

    /// Drop ignored and unselected findings (counted per code, never silent) and apply severity overrides.
    pub fn apply(
        &self,
        dataset: &Path,
        issues: Vec<Issue>,
    ) -> (Vec<Issue>, BTreeMap<String, usize>) {
        let per = self.dataset_ignores(dataset);
        let mut ignored = BTreeMap::new();
        let mut kept = Vec::new();
        for mut i in issues {
            let selected = self
                .select
                .as_ref()
                .is_none_or(|s| s.iter().any(|c| c == i.code));
            if !selected || self.ignore.iter().any(|c| c == i.code) || per.contains(i.code) {
                *ignored.entry(i.code.to_string()).or_insert(0) += 1;
                continue;
            }
            if self.error.iter().any(|c| c == i.code) {
                i.severity = Severity::Error;
            }
            if self.warn.iter().any(|c| c == i.code) {
                i.severity = Severity::Warning;
            }
            kept.push(i);
        }
        (kept, ignored)
    }
}
