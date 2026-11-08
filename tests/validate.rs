//! Smoke tests on synthetic LeRobot v2.1-style datasets written to a temp dir.
use arrow_array::{ArrayRef, FixedSizeListArray, Float32Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use lecop::{dataset::Dataset, validate::{Issue, Severity, validate}};
use std::{fs, path::Path, sync::Arc};

const FPS: f32 = 10.0;
const DIM: i32 = 2;

struct Ep { len: usize, ts: Vec<f32>, action: Vec<f32>, with_video: bool, with_data: bool }

/// A healthy episode: perfect timestamps, varying action.
fn good(len: usize) -> Ep {
    Ep {
        len,
        ts: (0..len).map(|i| i as f32 / FPS).collect(),
        action: (0..len * DIM as usize).map(|i| i as f32 * 0.1).collect(),
        with_video: true,
        with_data: true,
    }
}

fn fsl(vals: Vec<f32>) -> ArrayRef {
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    Arc::new(FixedSizeListArray::new(item, DIM, Arc::new(Float32Array::from(vals)), None))
}

fn write_ds(root: &Path, eps: &[Ep], claimed_total_frames: Option<usize>) {
    for d in ["meta", "data/chunk-000", "videos/chunk-000/observation.images.top"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    let frames: usize = eps.iter().map(|e| e.len).sum();
    let info = serde_json::json!({
        "codebase_version": "v2.1", "robot_type": "test", "fps": FPS,
        "total_episodes": eps.len(), "total_frames": claimed_total_frames.unwrap_or(frames),
        "chunks_size": 1000,
        "data_path": "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
        "video_path": "videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4",
        "features": {
            "action": {"dtype": "float32", "shape": [DIM]},
            "observation.images.top": {"dtype": "video", "shape": [4, 4, 3]},
            "timestamp": {"dtype": "float32", "shape": [1]},
        }
    });
    fs::write(root.join("meta/info.json"), info.to_string()).unwrap();
    let jsonl: String = eps.iter().enumerate()
        .map(|(i, e)| format!("{}\n", serde_json::json!({"episode_index": i, "length": e.len, "tasks": ["t"]})))
        .collect();
    fs::write(root.join("meta/episodes.jsonl"), jsonl).unwrap();

    for (i, e) in eps.iter().enumerate() {
        if e.with_video {
            fs::write(root.join(format!("videos/chunk-000/observation.images.top/episode_{i:06}.mp4")), b"").unwrap();
        }
        if !e.with_data { continue; }
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Float32, false),
            Field::new("frame_index", DataType::Int64, false),
            Field::new("action", DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM), false),
        ]));
        let batch = RecordBatch::try_new(schema.clone(), vec![
            Arc::new(Float32Array::from(e.ts.clone())),
            Arc::new(Int64Array::from((0..e.ts.len() as i64).collect::<Vec<_>>())),
            fsl(e.action.clone()),
        ]).unwrap();
        let mut w = ArrowWriter::try_new(fs::File::create(root.join(format!("data/chunk-000/episode_{i:06}.parquet"))).unwrap(), schema, None).unwrap();
        w.write(&batch).unwrap();
        w.close().unwrap();
    }
}

fn run(eps: Vec<Ep>, claimed_frames: Option<usize>) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &eps, claimed_frames);
    validate(&Dataset::open(dir.path()).unwrap()).unwrap()
}
fn codes(v: &[Issue]) -> Vec<&'static str> { v.iter().map(|i| i.code).collect() }

#[test]
fn healthy_dataset_has_no_issues() {
    let v = run(vec![good(20), good(15)], None);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn flags_timestamp_drift() {
    let mut e = good(20);
    for t in e.ts.iter_mut().skip(10) { *t += 0.05; } // 50 ms late from frame 10
    assert_eq!(codes(&run(vec![e], None)), ["timestamp_drift"]);
}

#[test]
fn flags_non_monotonic_timestamps() {
    let mut e = good(20);
    e.ts[7] = e.ts[5];
    let c = codes(&run(vec![e], None));
    assert!(c.contains(&"timestamp_not_monotonic"), "{c:?}");
}

#[test]
fn flags_nan_in_action() {
    let mut e = good(20);
    e.action[5] = f32::NAN;
    assert_eq!(codes(&run(vec![e], None)), ["non_finite"]);
}

#[test]
fn warns_on_frozen_action() {
    let mut e = good(20);
    e.action = vec![0.5; 20 * DIM as usize];
    let v = run(vec![e], None);
    assert_eq!(codes(&v), ["action_constant"]);
    assert_eq!(v[0].severity, Severity::Warning);
}

#[test]
fn flags_missing_video_and_data_files() {
    let mut a = good(10); a.with_video = false;
    let mut b = good(10); b.with_data = false;
    let c = codes(&run(vec![a, b], None));
    assert!(c.contains(&"video_missing") && c.contains(&"data_missing"), "{c:?}");
}

#[test]
fn flags_total_frames_mismatch() {
    assert_eq!(codes(&run(vec![good(10)], Some(99))), ["frame_count"]);
}

#[test]
fn rejects_v3_clearly() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("meta")).unwrap();
    fs::write(dir.path().join("meta/info.json"), r#"{"codebase_version":"v3.0","fps":30,"total_episodes":0,"total_frames":0,"data_path":"x","features":{}}"#).unwrap();
    fs::write(dir.path().join("meta/episodes.jsonl"), "").unwrap();
    let err = Dataset::open(dir.path()).err().unwrap().to_string();
    assert!(err.contains("v3"), "{err}");
}
