use lecop::discover::find_datasets;
use std::{fs, path::Path};

fn fake_dataset(root: &Path) {
    fs::create_dir_all(root.join("meta")).unwrap();
    fs::write(root.join("meta/info.json"), "{}").unwrap();
}

#[test]
fn a_dataset_directory_is_its_own_root() {
    let d = tempfile::tempdir().unwrap();
    fake_dataset(d.path());
    assert_eq!(find_datasets(d.path()).unwrap(), [d.path().to_path_buf()]);
}

#[test]
fn a_directory_of_datasets_yields_each_sorted_by_path() {
    let d = tempfile::tempdir().unwrap();
    for n in ["b", "a", "nested/c"] {
        fake_dataset(&d.path().join(n));
    }
    let found = find_datasets(d.path()).unwrap();
    let names: Vec<_> = found
        .iter()
        .map(|p| {
            p.strip_prefix(d.path())
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(names, ["a", "b", "nested/c"]);
}

#[test]
fn an_empty_directory_has_no_datasets() {
    let d = tempfile::tempdir().unwrap();
    assert!(find_datasets(d.path()).unwrap().is_empty());
}

#[test]
fn gitignored_datasets_are_skipped_without_a_git_repo() {
    let d = tempfile::tempdir().unwrap();
    fake_dataset(&d.path().join("keep"));
    fake_dataset(&d.path().join("skipme"));
    fs::write(d.path().join(".gitignore"), "skipme/\n").unwrap();
    let found = find_datasets(d.path()).unwrap();
    assert_eq!(found, [d.path().join("keep")]);
}

#[test]
fn a_path_that_does_not_exist_is_an_error_naming_it() {
    let e = find_datasets(Path::new("/definitely/not/here")).unwrap_err();
    assert!(format!("{e:#}").contains("/definitely/not/here"), "{e:#}");
}
