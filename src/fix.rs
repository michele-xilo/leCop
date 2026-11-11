//! `--fix`: the only repairs lecop makes are recomputing derived totals in `meta/info.json` from metadata that
//! stays untouched. Data and video files are never edited and nothing is invented.
use crate::{dataset::Dataset, validate::Issue};
use anyhow::{Context, Result, bail};
use std::{fs, path::Path};

/// One number in info.json that disagrees with the metadata it is derived from.
#[derive(Debug, PartialEq, Eq)]
pub struct Fix {
    pub key: &'static str,
    pub from: usize,
    pub to: usize,
    /// The finding this repairs.
    pub code: &'static str,
}

/// Findings that mean the episode metadata and the data files disagree: a total recomputed from the metadata
/// could then be as wrong as the one it replaces, so nothing is repaired.
const UNSAFE: [&str; 9] = [
    "episode_index",
    "range_length",
    "range_gap",
    "index_mismatch",
    "data_missing",
    "data_unreadable",
    "length_mismatch",
    "episode_rows_missing",
    "column_missing",
];

/// The repairs that are safe for `ds`, given the findings of a validation run (before any config filtering) and
/// the number of tasks the task metadata lists.
pub fn plan(ds: &Dataset, issues: &[Issue], tasks_found: Option<usize>) -> Vec<Fix> {
    let has = |code: &str| issues.iter().any(|i| i.code == code);
    if UNSAFE.iter().any(|c| has(c)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    if has("frame_count") {
        let to = ds.episodes.iter().map(|e| e.length).sum();
        out.push(Fix {
            key: "total_frames",
            from: ds.info.total_frames,
            to,
            code: "frame_count",
        });
    }
    if has("episode_count") {
        out.push(Fix {
            key: "total_episodes",
            from: ds.info.total_episodes,
            to: ds.episodes.len(),
            code: "episode_count",
        });
    }
    if let (true, Some(from), Some(to)) = (has("tasks_count"), ds.info.total_tasks, tasks_found) {
        out.push(Fix {
            key: "total_tasks",
            from,
            to,
            code: "tasks_count",
        });
    }
    out
}

/// Write `fixes` into `<root>/meta/info.json`, after copying the original to `info.json.bak`. Only the numbers
/// change: key order and formatting stay as they were. Refuses if a backup is already there.
pub fn apply(root: &Path, fixes: &[Fix]) -> Result<()> {
    let path = root.join("meta/info.json");
    let backup = root.join("meta/info.json.bak");
    if backup.exists() {
        bail!(
            "{} already exists; move it away before repairing again",
            backup.display()
        );
    }
    let text = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut fixed = text.clone();
    for f in fixes {
        fixed = replace_number(&fixed, f.key, f.from, f.to).with_context(|| {
            format!(
                "`{}` is not a single number {} in {}",
                f.key,
                f.from,
                path.display()
            )
        })?;
    }
    fs::copy(&path, &backup).with_context(|| format!("writing {}", backup.display()))?;
    // Write beside the file and rename, so a crash never leaves a half-written info.json.
    let tmp = root.join("meta/info.json.lecop-tmp");
    fs::write(&tmp, fixed).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, &path).with_context(|| format!("replacing {}", path.display()))
}

/// Replace the number `from` after the key `"key":` with `to`, or `None` unless exactly one such pair exists.
fn replace_number(text: &str, key: &str, from: usize, to: usize) -> Option<String> {
    let needle = format!("\"{key}\"");
    let mut found = None;
    for (at, _) in text.match_indices(&needle) {
        let rest = &text[at + needle.len()..];
        let Some(after_colon) = rest.trim_start().strip_prefix(':') else {
            continue;
        };
        let value = after_colon.trim_start();
        let digits = value.bytes().take_while(u8::is_ascii_digit).count();
        let ends_number = !matches!(value.as_bytes().get(digits), Some(b'.' | b'e' | b'E'));
        if digits > 0 && ends_number && value[..digits] == from.to_string() {
            if found.is_some() {
                return None;
            }
            found = Some(text.len() - value.len());
        }
    }
    let start = found?;
    Some(format!(
        "{}{to}{}",
        &text[..start],
        &text[start + from.to_string().len()..]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_the_number_and_nothing_else() {
        let t = "{\n  \"a\": 1,\n  \"total_frames\" :  19028,\n  \"b\": 19028\n}";
        assert_eq!(
            replace_number(t, "total_frames", 19028, 17156).unwrap(),
            "{\n  \"a\": 1,\n  \"total_frames\" :  17156,\n  \"b\": 19028\n}"
        );
        assert_eq!(
            replace_number("{\"total_frames\":5}", "total_frames", 5, 12).unwrap(),
            "{\"total_frames\":12}"
        );
    }

    #[test]
    fn refuses_when_the_value_is_not_the_expected_number() {
        assert!(replace_number("{\"total_frames\": 6}", "total_frames", 5, 1).is_none());
        assert!(replace_number("{\"total_frames\": 50}", "total_frames", 5, 1).is_none());
        assert!(replace_number("{\"total_frames\": 5.5}", "total_frames", 5, 1).is_none());
        assert!(replace_number("{}", "total_frames", 5, 1).is_none());
    }

    #[test]
    fn refuses_when_the_key_appears_twice_with_that_number() {
        let t = "{\"total_frames\": 5, \"x\": {\"total_frames\": 5}}";
        assert!(replace_number(t, "total_frames", 5, 1).is_none());
    }

    #[test]
    fn apply_fails_and_writes_nothing_when_the_number_is_not_there() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("meta")).unwrap();
        let info = dir.path().join("meta/info.json");
        std::fs::write(&info, "{\"total_frames\": 6}").unwrap();
        let fix = Fix {
            key: "total_frames",
            from: 5,
            to: 1,
            code: "total_frames",
        };
        let e = apply(dir.path(), &[fix]).unwrap_err();
        assert!(format!("{e:#}").contains("not a single number"), "{e:#}");
        assert_eq!(
            std::fs::read_to_string(&info).unwrap(),
            "{\"total_frames\": 6}"
        );
        assert!(!dir.path().join("meta/info.json.bak").exists());
    }

    #[test]
    fn ignores_the_key_inside_a_longer_name_or_as_a_value() {
        let t = "{\"total_frames_x\": 5, \"k\": \"total_frames\", \"total_frames\": 5}";
        assert_eq!(
            replace_number(t, "total_frames", 5, 7).unwrap(),
            "{\"total_frames_x\": 5, \"k\": \"total_frames\", \"total_frames\": 7}"
        );
    }
}
