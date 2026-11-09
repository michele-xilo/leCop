use lecop::{
    report::{DatasetReport, Format, Options, exit_code, render},
    validate::{Issue, Severity},
};
use std::path::PathBuf;

fn issue(severity: Severity, episode: Option<usize>, code: &'static str, message: &str) -> Issue {
    Issue {
        severity,
        episode,
        code,
        message: message.into(),
    }
}

fn sample() -> Vec<Issue> {
    vec![
        issue(Severity::Error, None, "frame_count", "info.json says 5"),
        issue(Severity::Warning, Some(3), "idle_episode", "60% idle"),
    ]
}

fn one(issues: Vec<Issue>) -> Vec<DatasetReport> {
    vec![DatasetReport::new(PathBuf::from("ds/a"), issues)]
}

fn opts(format: Format) -> Options {
    Options {
        format,
        color: false,
        statistics: false,
        quiet: false,
    }
}

#[test]
fn text_format_keeps_the_issue_lines_and_ends_with_a_summary() {
    let out = render(&one(sample()), &opts(Format::Text));
    assert_eq!(
        out,
        "ERROR   ep -      frame_count              info.json says 5\n\
         WARNING ep 3      idle_episode             60% idle\n\
         Found 1 error and 1 warning in 1 dataset.\n"
    );
}

#[test]
fn a_clean_run_says_all_checks_passed() {
    assert_eq!(
        render(&one(vec![]), &opts(Format::Text)),
        "All checks passed!\n"
    );
}

#[test]
fn several_datasets_prefix_each_line_with_the_path_and_count_datasets_with_findings() {
    let reports = vec![
        DatasetReport::new(PathBuf::from("ds/a"), sample()),
        DatasetReport::new(PathBuf::from("ds/b"), vec![]),
        DatasetReport::new(
            PathBuf::from("ds/c"),
            vec![issue(Severity::Error, Some(1), "frame_index", "bad")],
        ),
    ];
    let out = render(&reports, &opts(Format::Text));
    assert!(out.contains("ds/a: ERROR   ep -      frame_count"), "{out}");
    assert!(out.contains("ds/c: ERROR   ep 1      frame_index"), "{out}");
    assert!(
        out.ends_with("Found 2 errors and 1 warning in 2 datasets.\n"),
        "{out}"
    );
}

#[test]
fn quiet_prints_only_the_summary_and_nothing_on_success() {
    let mut o = opts(Format::Text);
    o.quiet = true;
    assert_eq!(
        render(&one(sample()), &o),
        "Found 1 error and 1 warning in 1 dataset.\n"
    );
    assert_eq!(render(&one(vec![]), &o), "");
}

#[test]
fn statistics_count_per_code_sorted_by_count_then_code() {
    let mut issues = sample();
    issues.push(issue(Severity::Warning, Some(4), "idle_episode", "x"));
    issues.push(issue(Severity::Warning, Some(5), "action_constant", "x"));
    let mut o = opts(Format::Text);
    o.statistics = true;
    let out = render(&one(issues), &o);
    let stats: Vec<&str> = out
        .lines()
        .filter(|l| l.starts_with("    ") && !l.contains("ep "))
        .collect();
    assert_eq!(
        stats,
        [
            "    2  idle_episode",
            "    1  action_constant",
            "    1  frame_count"
        ],
        "{out}"
    );
}

#[test]
fn ignored_findings_are_reported_not_silent() {
    let mut r = DatasetReport::new(PathBuf::from("ds/a"), vec![]);
    r.ignored.insert("video_fps".into(), 2);
    r.ignored.insert("idle_episode".into(), 5);
    let out = render(&[r], &opts(Format::Text));
    assert!(out.contains("All checks passed!"), "{out}");
    assert!(
        out.contains("Ignored by config: idle_episode (5), video_fps (2)"),
        "{out}"
    );
}

#[test]
fn full_format_adds_the_dataset_episode_and_an_explanation_per_issue() {
    let out = render(&one(sample()), &opts(Format::Full));
    assert!(out.contains("        --> ds/a, episode 3\n"), "{out}");
    assert!(out.contains("        --> ds/a\n"), "{out}"); // dataset-wide issue has no episode
    assert!(out.contains("        help: "), "{out}");
}

#[test]
fn every_issue_code_has_an_explanation() {
    for code in lecop::validate::ALL_CODES {
        assert!(!lecop::validate::explain(code).is_empty(), "{code}");
    }
}

#[test]
fn colour_wraps_severity_and_summary_only_when_enabled() {
    let mut o = opts(Format::Text);
    o.color = true;
    let out = render(&one(sample()), &o);
    assert!(out.contains("\x1b[31mERROR\x1b[0m"), "{out:?}");
    assert!(out.contains("\x1b[33mWARNING\x1b[0m"), "{out:?}");
    assert!(!render(&one(sample()), &opts(Format::Text)).contains('\x1b'));
    let ok = render(&one(vec![]), &o);
    assert_eq!(ok, "\x1b[32mAll checks passed!\x1b[0m\n");
}

#[test]
fn json_has_stable_fields_counts_ignored_and_the_dataset_of_each_issue() {
    let mut r = DatasetReport::new(PathBuf::from("ds/a"), sample());
    r.ignored.insert("video_fps".into(), 2);
    let out = render(&[r], &opts(Format::Json));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["errors"], 1);
    assert_eq!(v["warnings"], 1);
    assert_eq!(v["ignored"], serde_json::json!({"video_fps": 2}));
    assert_eq!(
        v["issues"][0],
        serde_json::json!({"dataset":"ds/a","severity":"error","episode":null,"code":"frame_count","message":"info.json says 5"})
    );
    assert_eq!(v["issues"][1]["episode"], 3);
}

#[test]
fn json_of_a_healthy_dataset_is_an_empty_list() {
    let v: serde_json::Value =
        serde_json::from_str(&render(&one(vec![]), &opts(Format::Json))).unwrap();
    assert_eq!(v["issues"], serde_json::json!([]));
    assert_eq!(v["errors"], 0);
}

#[test]
fn github_format_emits_workflow_commands_and_escapes() {
    let out = render(&one(sample()), &opts(Format::Github));
    assert!(
        out.starts_with("::error title=frame_count::info.json says 5\n"),
        "{out}"
    );
    assert!(
        out.contains("::warning title=idle_episode::episode 3: 60%25 idle\n"),
        "{out}"
    );
    let bad = render(
        &one(vec![issue(
            Severity::Error,
            None,
            "data_unreadable",
            "100%\nbad\r",
        )]),
        &opts(Format::Github),
    );
    assert!(
        bad.starts_with("::error title=data_unreadable::100%25%0Abad%0D\n"),
        "{bad}"
    );
}

#[test]
fn exit_code_is_0_clean_or_warnings_only_1_on_errors_2_when_a_dataset_cannot_be_opened() {
    assert_eq!(exit_code(&one(vec![])), 0);
    assert_eq!(
        exit_code(&one(vec![issue(
            Severity::Warning,
            None,
            "idle_episode",
            "x"
        )])),
        0
    );
    assert_eq!(exit_code(&one(sample())), 1);
    let mut broken = DatasetReport::new(PathBuf::from("ds/x"), vec![]);
    broken.open_error = Some("no info.json".into());
    assert_eq!(
        exit_code(&[broken.clone(), DatasetReport::new("ds/a".into(), sample())]),
        2
    );
    let out = render(&[broken], &opts(Format::Text));
    assert!(
        out.contains("ds/x: ERROR   cannot open dataset: no info.json"),
        "{out}"
    );
}
