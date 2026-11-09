//! Find LeRobot dataset roots (directories holding `meta/info.json`) under a path.
use anyhow::{Result, bail};
use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

/// `path` itself if it is a dataset, else every dataset below it, sorted by path. `.gitignore` is honoured
/// even outside a git repository, so a scan of a project directory skips what the project ignores.
pub fn find_datasets(path: &Path) -> Result<Vec<PathBuf>> {
    if path.join("meta/info.json").is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        bail!("{} is not a directory", path.display());
    }
    let mut found = Vec::new();
    for entry in WalkBuilder::new(path).require_git(false).build() {
        let entry = entry?;
        let p = entry.path();
        if p.file_name().is_some_and(|n| n == "info.json")
            && p.parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == "meta")
            && let Some(root) = p.parent().and_then(Path::parent)
        {
            found.push(root.to_path_buf());
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}
