use lecop::{
    config::Config,
    validate::{Issue, Severity},
};
use std::{fs, path::Path};

fn issue(severity: Severity, code: &'static str) -> Issue {
    Issue {
        severity,
        episode: Some(0),
        code,
        message: "m".into(),
    }
}

fn parse(toml: &str) -> anyhow::Result<Config> {
    let d = tempfile::tempdir()?;
    let p = d.path().join("lecop.toml");
    fs::write(&p, toml)?;
    Config::load(&p)
}

fn codes(v: &[Issue]) -> Vec<&'static str> {
    v.iter().map(|i| i.code).collect()
}

fn sample() -> Vec<Issue> {
    vec![
        issue(Severity::Warning, "idle_episode"),
        issue(Severity::Warning, "video_fps"),
        issue(Severity::Error, "frame_count"),
    ]
}

#[test]
fn ignore_drops_findings_and_counts_them() {
    let c = parse(r#"ignore = ["idle_episode"]"#).unwrap();
    let (kept, ignored) = c.apply(Path::new("ds"), sample());
    assert_eq!(codes(&kept), ["video_fps", "frame_count"]);
    assert_eq!(ignored["idle_episode"], 1);
}

#[test]
fn select_runs_only_the_listed_codes_and_counts_the_rest_as_ignored() {
    let c = parse(r#"select = ["frame_count"]"#).unwrap();
    let (kept, ignored) = c.apply(Path::new("ds"), sample());
    assert_eq!(codes(&kept), ["frame_count"]);
    assert_eq!(ignored.len(), 2);
}

#[test]
fn severity_overrides_promote_and_demote() {
    let c = parse(
        r#"error = ["idle_episode"]
warn = ["frame_count"]"#,
    )
    .unwrap();
    let (kept, _) = c.apply(Path::new("ds"), sample());
    assert_eq!(kept[0].severity, Severity::Error);
    assert_eq!(kept[2].severity, Severity::Warning);
}

#[test]
fn per_dataset_ignores_apply_only_to_that_dataset() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("lecop.toml");
    fs::write(&p, "[per-dataset.\"data/a\"]\nignore = [\"frame_count\"]\n").unwrap();
    fs::create_dir_all(d.path().join("data/a")).unwrap();
    fs::create_dir_all(d.path().join("data/b")).unwrap();
    let c = Config::load(&p).unwrap();
    let (a, _) = c.apply(&d.path().join("data/a"), sample());
    let (b, _) = c.apply(&d.path().join("data/b"), sample());
    assert_eq!(codes(&a), ["idle_episode", "video_fps"]);
    assert_eq!(codes(&b).len(), 3);
}

#[test]
fn an_unknown_code_is_an_error_that_names_it() {
    let e = parse(r#"ignore = ["idle_episod"]"#).unwrap_err();
    assert!(format!("{e:#}").contains("idle_episod"), "{e:#}");
}

#[test]
fn an_unknown_key_is_rejected() {
    assert!(parse("ignroe = []").is_err());
}

#[test]
fn cli_codes_are_validated_and_merged() {
    let mut c = Config::default();
    c.add_cli(&["video_fps".into()], &[]).unwrap();
    let (kept, _) = c.apply(Path::new("ds"), sample());
    assert_eq!(codes(&kept), ["idle_episode", "frame_count"]);
    assert!(c.add_cli(&["nope".into()], &[]).is_err());
}

#[test]
fn find_walks_up_to_the_nearest_lecop_toml() {
    let d = tempfile::tempdir().unwrap();
    fs::write(d.path().join("lecop.toml"), "").unwrap();
    let deep = d.path().join("a/b/c");
    fs::create_dir_all(&deep).unwrap();
    let found = Config::find(&deep).unwrap();
    assert_eq!(
        found.canonicalize().unwrap(),
        d.path().join("lecop.toml").canonicalize().unwrap()
    );
}

#[test]
fn limits_are_read_and_checked() {
    let c = parse("[limits.\"action\"]\nmin = -1.0\nmax = [1.0, 2.0]\nmax_step = 0.2\n").unwrap();
    assert!(c.limits().contains_key("action"));
    assert!(
        parse("[limits.\"action\"]\nmaxx = 1.0\n").is_err(),
        "unknown key"
    );
    assert!(
        parse("[limits.\"next.reward\"]\nmax = 1.0\n").is_err(),
        "only action and observation.*"
    );
    assert!(
        parse("[limits.\"observation.state\"]\nmax_step = 1.0\n").is_err(),
        "max_step is for the action"
    );
}

#[test]
fn the_training_chunk_size_is_read_and_checked() {
    assert_eq!(
        parse("[training]\nchunk_size = 100\n")
            .unwrap()
            .chunk_size(),
        Some(100)
    );
    assert_eq!(parse("").unwrap().chunk_size(), None);
    assert!(
        parse("[training]\nchunk_size = 0\n").is_err(),
        "must be positive"
    );
    assert!(parse("[training]\nchunks = 5\n").is_err(), "unknown key");
}
