use crate::dataset::Dataset;
use std::path::Path;

/// Templates that start at the filesystem root make `root.join(template)` ignore the dataset root, so the files
/// are read from wherever the template points; stray files in `data/` ship with the dataset to the Hub.
pub(super) fn portability_findings(ds: &Dataset) -> Vec<(&'static str, String)> {
    let templates = [
        ("data_path", Some(ds.info.data_path.as_str())),
        ("video_path", ds.info.video_path.as_deref()),
    ];
    let mut out: Vec<(&'static str, String)> = templates
        .into_iter()
        .filter_map(|(name, t)| Some((name, t?)))
        .filter(|(_, t)| is_absolute_template(t))
        .map(|(name, t)| {
            (
                "path_absolute",
                format!("{name} is the absolute path `{t}`"),
            )
        })
        .collect();
    let scan = scan_files(&ds.root, LARGE_FILE);
    if !scan.stray.is_empty() {
        out.push((
            "data_stray_file",
            format!(
                "data/ holds files that are not parquet: {}",
                list_some(&scan.stray)
            ),
        ));
    }
    // A Hub cache snapshot is made of symlinks into `blobs/` by design.
    if !scan.symlinks.is_empty() && !ds.root.components().any(|c| c.as_os_str() == "snapshots") {
        out.push((
            "symlink",
            format!(
                "symbolic links break when the dataset is copied or uploaded: {}",
                list_some(&scan.symlinks)
            ),
        ));
    }
    if !scan.large.is_empty() {
        out.push((
            "file_oversized",
            format!("files over 5 GB: {}", list_some(&scan.large)),
        ));
    }
    out
}

pub(super) fn is_absolute_template(t: &str) -> bool {
    let b = t.as_bytes();
    t.starts_with('/')
        || t.starts_with('\\')
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
}

/// Above this the Hub recommends splitting a file.
pub(super) const LARGE_FILE: u64 = 5 << 30;

/// At most five of `items`, then how many more there are.
pub(super) fn list_some(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(5).map(String::as_str).collect();
    match items.len().saturating_sub(shown.len()) {
        0 => shown.join(", "),
        more => format!("{} and {more} more", shown.join(", ")),
    }
}

/// What a walk over the dataset's files found; paths are relative to the root and sorted.
#[derive(Default)]
pub(super) struct Scan {
    /// Files below `data/` that are not parquet.
    pub(super) stray: Vec<String>,
    pub(super) symlinks: Vec<String>,
    /// Files of at least `limit` bytes, with their size.
    pub(super) large: Vec<String>,
}

/// Walk every file below `root` without following links.
pub(super) fn scan_files(root: &Path, limit: u64) -> Scan {
    let mut out = Scan::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let (p, Ok(kind)) = (e.path(), e.file_type()) else {
                continue;
            };
            let name = || p.strip_prefix(root).unwrap_or(&p).display().to_string();
            if kind.is_symlink() {
                out.symlinks.push(name());
            } else if kind.is_dir() {
                stack.push(p);
            } else {
                if p.extension().is_none_or(|x| x != "parquet") && p.starts_with(root.join("data"))
                {
                    out.stray.push(name());
                }
                if let Ok(m) = e.metadata()
                    && m.len() >= limit
                {
                    out.large.push(format!(
                        "{} ({:.1} GB)",
                        name(),
                        m.len() as f64 / (1u64 << 30) as f64
                    ));
                }
            }
        }
    }
    out.stray.sort();
    out.symlinks.sort();
    out.large.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanning_a_directory_that_cannot_be_read_finds_nothing() {
        let scan = scan_files(std::path::Path::new("no/such/dir"), 1);
        assert!(scan.stray.is_empty() && scan.symlinks.is_empty() && scan.large.is_empty());
    }

    #[test]
    fn absolute_templates_are_recognised() {
        assert!(is_absolute_template("/data/ep_{episode_index}.parquet"));
        assert!(is_absolute_template("C:\\data\\ep.parquet"));
        assert!(!is_absolute_template(
            "data/chunk-{episode_chunk:03d}/ep.parquet"
        ));
    }
}
