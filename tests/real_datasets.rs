//! Runs `validate` on every dataset from scripts/datasets.tsv that is present in `datasets/`, and checks the
//! error and warning counts. Absent datasets are skipped: fetch them with scripts/fetch-datasets.sh.
use lecop::{
    dataset::Dataset,
    validate::{Severity, validate},
};
use std::path::Path;

#[test]
fn real_datasets_report_the_expected_counts() {
    let manifest = std::fs::read_to_string("scripts/datasets.tsv").unwrap();
    let (mut ran, mut bad) = (0, Vec::new());
    for line in manifest.lines().filter(|l| !l.starts_with('#')) {
        let f: Vec<&str> = line.split('\t').collect();
        let (name, want) = (
            f[0],
            (
                f[3].parse::<usize>().unwrap(),
                f[4].parse::<usize>().unwrap(),
            ),
        );
        let path = Path::new("datasets").join(name);
        if !path.exists() {
            eprintln!("skipping {name}: not downloaded");
            continue;
        }
        let issues = validate(&Dataset::open(&path).unwrap()).unwrap();
        let errors = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count();
        let got = (errors, issues.len() - errors);
        ran += 1;
        if got != want {
            bad.push(format!(
                "{name}: expected {want:?} (errors, warnings), got {got:?}"
            ));
        }
    }
    eprintln!("checked {ran} real datasets");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
