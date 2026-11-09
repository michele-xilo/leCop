mod common;
use common::synth::{Synth, write_v2};
use std::process::{Command, Output};

fn lecop(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lecop"))
        .args(args)
        .output()
        .unwrap()
}

fn dataset() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let s = Synth {
        episodes: 3,
        frames: 40,
        dim: 2,
        fps: 30,
        per_file: 1,
    };
    write_v2(dir.path(), &s);
    dir
}

fn break_frame_total(dir: &tempfile::TempDir) {
    let p = dir.path().join("meta/info.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["total_frames"] = 1.into();
    std::fs::write(p, v.to_string()).unwrap();
}

#[test]
fn healthy_dataset_exits_zero_in_every_format() {
    let d = dataset();
    let path = d.path().to_str().unwrap();
    for f in ["text", "json", "github"] {
        let o = lecop(&["validate", path, "--format", f]);
        assert!(o.status.success(), "{f}: {o:?}");
    }
}

#[test]
fn json_output_is_parseable_and_exit_code_is_one_on_errors() {
    let d = dataset();
    break_frame_total(&d);
    let o = lecop(&["validate", d.path().to_str().unwrap(), "--format", "json"]);
    assert_eq!(o.status.code(), Some(1));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["issues"][0]["code"], "frame_count");
}

#[test]
fn unknown_format_is_rejected() {
    let o = lecop(&["validate", ".", "--format", "xml"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn inspect_prints_the_summary() {
    let d = dataset();
    let o = lecop(&["inspect", d.path().to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("episodes  3"));
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn lecop_in(dir: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_lecop"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

/// Two datasets, `a` healthy and `b` with a wrong frame total, under one parent.
fn parent_of_two() -> tempfile::TempDir {
    let parent = tempfile::tempdir().unwrap();
    for name in ["a", "b"] {
        let s = Synth {
            episodes: 3,
            frames: 40,
            dim: 2,
            fps: 30,
            per_file: 1,
        };
        std::fs::create_dir(parent.path().join(name)).unwrap();
        write_v2(&parent.path().join(name), &s);
    }
    let p = parent.path().join("b/meta/info.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["total_frames"] = 1.into();
    std::fs::write(p, v.to_string()).unwrap();
    parent
}

#[test]
fn no_argument_scans_the_current_directory() {
    let parent = parent_of_two();
    let o = lecop_in(parent.path(), &["validate"]);
    assert_eq!(o.status.code(), Some(1));
    let s = out(&o);
    assert!(s.contains("frame_count"), "{s}");
    assert!(
        s.contains("Found 1 error and 0 warnings in 1 dataset."),
        "{s}"
    );
}

#[test]
fn healthy_run_prints_all_checks_passed_and_quiet_prints_nothing() {
    let d = dataset();
    let path = d.path().to_str().unwrap();
    assert!(out(&lecop(&["validate", path])).contains("All checks passed!"));
    assert_eq!(out(&lecop(&["validate", path, "-q"])), "");
}

#[test]
fn empty_directory_exits_two_with_a_message() {
    let d = tempfile::tempdir().unwrap();
    let o = lecop(&["validate", d.path().to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no LeRobot datasets"));
}

#[test]
fn gitignored_datasets_are_skipped() {
    let parent = parent_of_two();
    std::fs::write(parent.path().join(".gitignore"), "b/\n").unwrap();
    let o = lecop_in(parent.path(), &["validate"]);
    assert_eq!(o.status.code(), Some(0), "{}", out(&o));
}

#[test]
fn colour_is_off_when_piped_and_with_no_color() {
    let d = dataset();
    break_frame_total(&d);
    let path = d.path().to_str().unwrap();
    assert!(!out(&lecop(&["validate", path])).contains('\u{1b}'));
    assert!(!out(&lecop(&["validate", path, "--no-color"])).contains('\u{1b}'));
}

#[test]
fn full_format_and_alias_add_location_and_help() {
    let d = dataset();
    break_frame_total(&d);
    let path = d.path().to_str().unwrap();
    let s = out(&lecop(&["validate", path, "--output-format", "full"]));
    assert!(s.contains("-->") && s.contains("help:"), "{s}");
}

#[test]
fn statistics_counts_findings_per_code() {
    let d = dataset();
    break_frame_total(&d);
    let s = out(&lecop(&[
        "validate",
        d.path().to_str().unwrap(),
        "--statistics",
    ]));
    assert!(s.contains("1  frame_count"), "{s}");
}

#[test]
fn report_is_identical_for_any_job_count() {
    let parent = parent_of_two();
    let p = parent.path().to_str().unwrap();
    let one = lecop(&["validate", p, "-j", "1"]);
    let many = lecop(&["validate", p, "-j", "8"]);
    assert_eq!(one.stdout, many.stdout);
    assert_eq!(one.status.code(), many.status.code());
}

#[test]
fn ignore_flag_and_config_file_drop_a_code_and_say_so() {
    let d = dataset();
    break_frame_total(&d);
    let path = d.path().to_str().unwrap();
    let o = lecop(&["validate", path, "--ignore", "frame_count"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(
        out(&o).contains("Ignored by config: frame_count (1)"),
        "{}",
        out(&o)
    );

    std::fs::write(d.path().join("lecop.toml"), "ignore = [\"frame_count\"]\n").unwrap();
    assert_eq!(lecop(&["validate", path]).status.code(), Some(0));
    assert_eq!(
        lecop(&["validate", path, "--isolated"]).status.code(),
        Some(1)
    );
}

#[test]
fn select_flag_keeps_only_the_named_codes() {
    let d = dataset();
    break_frame_total(&d);
    let o = lecop(&[
        "validate",
        d.path().to_str().unwrap(),
        "--select",
        "range_gap",
    ]);
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn unknown_code_in_flag_or_config_exits_two() {
    let d = dataset();
    let path = d.path().to_str().unwrap();
    assert_eq!(
        lecop(&["validate", path, "--ignore", "nope"]).status.code(),
        Some(2)
    );
    std::fs::write(d.path().join("lecop.toml"), "ignore = [\"nope\"]\n").unwrap();
    let o = lecop(&["validate", path]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("unknown issue code"));
}

#[test]
fn logging_is_silent_by_default_and_goes_to_stderr_with_v() {
    let d = dataset();
    let path = d.path().to_str().unwrap();
    let quiet = lecop(&["validate", path]);
    assert!(quiet.stderr.is_empty(), "{:?}", quiet.stderr);

    let v = lecop(&["validate", path, "-v"]);
    assert!(String::from_utf8_lossy(&v.stderr).contains("validated"));
    assert_eq!(v.stdout, quiet.stdout);

    let vv = lecop(&["validate", path, "-vv"]);
    assert!(String::from_utf8_lossy(&vv.stderr).contains("read data file"));
}

#[test]
fn rust_log_overrides_the_verbosity_flag() {
    let d = dataset();
    let o = Command::new(env!("CARGO_BIN_EXE_lecop"))
        .env("RUST_LOG", "off")
        .args(["validate", d.path().to_str().unwrap(), "-vv"])
        .output()
        .unwrap();
    assert!(o.stderr.is_empty());
}
