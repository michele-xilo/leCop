//! Runs `validate` on every dataset from scripts/datasets.tsv that is present in `datasets/`, and checks the
//! error and warning counts. Absent datasets are skipped: fetch them with scripts/fetch-datasets.sh.
use lecop::{
    dataset::Dataset,
    validate::{Severity, validate},
};
use std::{collections::BTreeMap, path::Path};

#[test]
fn real_datasets_report_the_expected_counts_and_codes() {
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
        let want_codes = parse_codes(f[5]);
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
        let mut got_codes = BTreeMap::new();
        for i in &issues {
            *got_codes.entry(i.code.to_string()).or_insert(0) += 1;
        }
        ran += 1;
        if got_codes != want_codes {
            bad.push(format!(
                "{name}: expected codes {want_codes:?}, got {got_codes:?}"
            ));
        }
        if got != want {
            bad.push(format!(
                "{name}: expected {want:?} (errors, warnings), got {got:?}"
            ));
        }
    }
    eprintln!("checked {ran} real datasets");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// `code=count code=count`, or `-` for none.
fn parse_codes(field: &str) -> BTreeMap<String, usize> {
    field
        .split_whitespace()
        .filter(|t| *t != "-")
        .map(|t| {
            let (code, n) = t.split_once('=').unwrap();
            (code.to_string(), n.parse().unwrap())
        })
        .collect()
}
