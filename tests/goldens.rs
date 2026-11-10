//! Replays what lerobot-doctor and physlint reported on the datasets in `datasets/` (tests/goldens/*.json, recorded
//! by scripts/goldens.py) against lecop, through the mapping in scripts/golden_map.tsv. Datasets that are not
//! downloaded are skipped, so a clean clone passes.
use lecop::{
    dataset::Dataset,
    validate::{Issue, Severity, validate},
};
use serde::Deserialize;
use std::{collections::HashMap, fs, path::Path};

#[derive(Deserialize)]
struct Golden {
    dataset: String,
    tools: HashMap<String, ToolRun>,
}

#[derive(Deserialize)]
struct ToolRun {
    findings: Vec<Finding>,
}

#[derive(Deserialize)]
struct Finding {
    kind: String,
    episode: Option<usize>,
}

struct Mapping {
    codes: Vec<String>,
    status: String,
    note: String,
}

type Kinds = HashMap<(String, String), Mapping>;
type Exceptions = HashMap<(String, String, String), String>;

fn read_map() -> (Kinds, Exceptions, HashMap<(String, String), String>) {
    let text = fs::read_to_string("scripts/golden_map.tsv").unwrap();
    let (mut kinds, mut known_miss, mut lecop_only) =
        (HashMap::new(), HashMap::new(), HashMap::new());
    for line in text
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        if f[0] == "lecop-only" {
            lecop_only.insert((f[1].to_string(), f[2].to_string()), f[3].to_string());
            continue;
        }
        if f[0] == "known_miss" {
            known_miss.insert((f[1].into(), f[2].into(), f[3].into()), f[4].into());
            continue;
        }
        kinds.insert(
            (f[0].to_string(), f[1].to_string()),
            Mapping {
                codes: f[2]
                    .split(',')
                    .filter(|c| *c != "-")
                    .map(String::from)
                    .collect(),
                status: f[3].to_string(),
                note: f.get(4).unwrap_or(&"").to_string(),
            },
        );
    }
    (kinds, known_miss, lecop_only)
}

fn reported(issues: &[Issue], codes: &[String], episode: Option<usize>) -> bool {
    issues
        .iter()
        .any(|i| codes.iter().any(|c| c == i.code) && (episode.is_none() || i.episode == episode))
}

#[test]
fn lecop_reports_what_the_other_tools_report() {
    let (map, known_miss, lecop_only) = read_map();
    let (mut matched, mut known, mut gaps, mut skipped_out_of_scope) = (0, 0, Vec::new(), 0);
    let (mut missing, mut unexplained, mut datasets) = (Vec::new(), Vec::new(), 0);
    let mut dir: Vec<_> = fs::read_dir("tests/goldens")
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    dir.sort();
    for file in dir {
        let golden: Golden = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        let path = Path::new("datasets").join(&golden.dataset);
        if !path.exists() {
            eprintln!("skipping {}: not downloaded", golden.dataset);
            continue;
        }
        datasets += 1;
        let issues = validate(&Dataset::open(&path).unwrap()).unwrap();
        let mut explained: Vec<&str> = Vec::new();
        for (tool, run) in &golden.tools {
            for f in &run.findings {
                let Some(m) = map.get(&(tool.clone(), f.kind.clone())) else {
                    missing.push(format!(
                        "{}: {tool} {} is not in scripts/golden_map.tsv",
                        golden.dataset, f.kind
                    ));
                    continue;
                };
                match m.status.as_str() {
                    "matched" if reported(&issues, &m.codes, f.episode) => {
                        matched += 1;
                        explained.extend(m.codes.iter().map(String::as_str));
                    }
                    "matched"
                        if known_miss.contains_key(&(
                            golden.dataset.clone(),
                            tool.clone(),
                            f.kind.clone(),
                        )) =>
                    {
                        known += 1;
                    }
                    "matched" => missing.push(format!(
                        "{}: {tool} {} (episode {:?}) but lecop reports none of {:?}",
                        golden.dataset, f.kind, f.episode, m.codes
                    )),
                    "gap" => {
                        gaps.push(format!("{} {tool} {} ({})", golden.dataset, f.kind, m.note))
                    }
                    _ => skipped_out_of_scope += 1,
                }
            }
        }
        // Every error lecop reports must be one the other tools also report, or be explained in the map.
        for i in issues.iter().filter(|i| i.severity == Severity::Error) {
            let known = explained.contains(&i.code)
                || lecop_only.contains_key(&(golden.dataset.clone(), i.code.to_string()));
            if !known {
                unexplained.push(format!(
                    "{}: lecop error {} ({})",
                    golden.dataset, i.code, i.message
                ));
            }
        }
    }
    gaps.sort();
    gaps.dedup();
    eprintln!(
        "{datasets} datasets: {matched} findings matched, {known} known misses, {} gap findings, {skipped_out_of_scope} out of scope",
        gaps.len()
    );
    for g in &gaps {
        eprintln!("expected miss: {g}");
    }
    let problems: Vec<String> = missing.into_iter().chain(unexplained).collect();
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn every_mapped_code_is_a_real_code_listed_in_the_readme() {
    let (map, _, _) = read_map();
    let readme = fs::read_to_string("README.md").unwrap();
    for m in map.values().filter(|m| m.status == "matched") {
        for c in &m.codes {
            assert!(
                lecop::validate::ALL_CODES.contains(&c.as_str()),
                "{c} is not an issue code"
            );
            assert!(
                readme.contains(&format!("`{c}`")),
                "README does not list `{c}`"
            );
        }
    }
}
