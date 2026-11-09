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
        frames: 20,
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
