//! Smoke tests on synthetic LeRobot v2.1-style datasets written to a temp dir.
mod common;
use arrow_array::{ArrayRef, FixedSizeListArray, Float32Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use common::Mp4;
use lecop::{
    dataset::Dataset,
    validate::{Issue, Severity, validate},
};
use parquet::arrow::ArrowWriter;
use std::{fs, path::Path, sync::Arc};

const FPS: f32 = 10.0;
const DIM: i32 = 2;

struct Ep {
    len: usize,
    ts: Vec<f32>,
    action: Vec<f32>,
    with_video: bool,
    with_data: bool,
    range_to_delta: i64,
    /// v2: video has `len + delta` frames. v3: `to_timestamp` is `delta` frames past the real end.
    video_delta: i32,
    video_fps: u32,
    video_garbage: bool,
}

fn video_bytes(frames: i64, e: &Ep) -> Vec<u8> {
    if e.video_garbage {
        return b"not an mp4".to_vec();
    }
    Mp4::constant_fps(frames.max(0) as u32, e.video_fps).bytes()
}

/// A healthy episode: perfect timestamps, varying action.
fn good(len: usize) -> Ep {
    Ep {
        len,
        ts: (0..len).map(|i| i as f32 / FPS).collect(),
        action: (0..len * DIM as usize).map(|i| i as f32 * 0.1).collect(),
        with_video: true,
        with_data: true,
        range_to_delta: 0,
        video_delta: 0,
        video_fps: FPS as u32,
        video_garbage: false,
    }
}

fn fsl(vals: Vec<f32>) -> ArrayRef {
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    Arc::new(FixedSizeListArray::new(
        item,
        DIM,
        Arc::new(Float32Array::from(vals)),
        None,
    ))
}

fn write_ds(root: &Path, eps: &[Ep], claimed_total_frames: Option<usize>) {
    for d in [
        "meta",
        "data/chunk-000",
        "videos/chunk-000/observation.images.top",
    ] {
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
    let jsonl: String = eps
        .iter()
        .enumerate()
        .map(|(i, e)| {
            format!(
                "{}\n",
                serde_json::json!({"episode_index": i, "length": e.len, "tasks": ["t"]})
            )
        })
        .collect();
    fs::write(root.join("meta/episodes.jsonl"), jsonl).unwrap();

    for (i, e) in eps.iter().enumerate() {
        if e.with_video {
            fs::write(
                root.join(format!(
                    "videos/chunk-000/observation.images.top/episode_{i:06}.mp4"
                )),
                video_bytes(e.len as i64 + e.video_delta as i64, e),
            )
            .unwrap();
        }
        if !e.with_data {
            continue;
        }
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Float32, false),
            Field::new("frame_index", DataType::Int64, false),
            Field::new(
                "action",
                DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
                false,
            ),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Float32Array::from(e.ts.clone())),
                Arc::new(Int64Array::from((0..e.ts.len() as i64).collect::<Vec<_>>())),
                fsl(e.action.clone()),
            ],
        )
        .unwrap();
        let mut w = ArrowWriter::try_new(
            fs::File::create(root.join(format!("data/chunk-000/episode_{i:06}.parquet"))).unwrap(),
            schema,
            None,
        )
        .unwrap();
        w.write(&batch).unwrap();
        w.close().unwrap();
    }
}

fn run(eps: Vec<Ep>, claimed_frames: Option<usize>) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &eps, claimed_frames);
    validate(&Dataset::open(dir.path()).unwrap()).unwrap()
}
fn codes(v: &[Issue]) -> Vec<&'static str> {
    v.iter().map(|i| i.code).collect()
}

#[test]
fn healthy_dataset_has_no_issues() {
    let v = run(vec![good(20), good(15)], None);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn flags_timestamp_drift() {
    let mut e = good(20);
    for t in e.ts.iter_mut().skip(10) {
        *t += 0.05;
    } // 50 ms late from frame 10
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
    let mut a = good(10);
    a.with_video = false;
    let mut b = good(10);
    b.with_data = false;
    let c = codes(&run(vec![a, b], None));
    assert!(
        c.contains(&"video_missing") && c.contains(&"data_missing"),
        "{c:?}"
    );
}

#[test]
fn flags_total_frames_mismatch() {
    assert_eq!(codes(&run(vec![good(10)], Some(99))), ["frame_count"]);
}

#[test]
fn rejects_unknown_future_version() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("meta")).unwrap();
    fs::write(dir.path().join("meta/info.json"), r#"{"codebase_version":"v9.0","fps":30,"total_episodes":0,"total_frames":0,"data_path":"x","features":{}}"#).unwrap();
    let err = Dataset::open(dir.path()).err().unwrap().to_string();
    assert!(err.contains("v9.0"), "{err}");
}

// ---------------------------------------------------------------- v3.0 (packed files)

use arrow_array::{Float64Array, StringArray};

const PER_FILE: usize = 2; // episodes packed per data/video file

fn i64s(v: Vec<i64>) -> ArrayRef {
    Arc::new(Int64Array::from(v))
}

/// v3.0 layout: PER_FILE episodes per data/video file, episode boundaries in meta/episodes parquet.
/// Timestamps in data files are episode-relative.
fn write_ds_v3(root: &Path, eps: &[Ep]) {
    let vk = "observation.images.top";
    for d in [
        "meta/episodes/chunk-000",
        "data/chunk-000",
        &format!("videos/{vk}/chunk-000"),
    ] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    let frames: usize = eps.iter().map(|e| e.len).sum();
    let info = serde_json::json!({
        "codebase_version": "v3.0", "robot_type": "test", "fps": FPS,
        "total_episodes": eps.len(), "total_frames": frames,
        "data_path": "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
        "video_path": "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
        "features": {
            "action": {"dtype": "float32", "shape": [DIM]},
            "observation.images.top": {"dtype": "video", "shape": [4, 4, 3]},
            "timestamp": {"dtype": "float32", "shape": [1]},
        }
    });
    fs::write(root.join("meta/info.json"), info.to_string()).unwrap();

    let (mut from, mut cols): (i64, [Vec<i64>; 4]) = (0, Default::default()); // ep, len, from, to
    let mut file_idx = Vec::new();
    let (mut vfrom, mut vto) = (Vec::<f64>::new(), Vec::<f64>::new());
    for (g, group) in eps.chunks(PER_FILE).enumerate() {
        if group[0].with_video {
            fs::write(
                root.join(format!("videos/{vk}/chunk-000/file-{g:03}.mp4")),
                video_bytes(group.iter().map(|e| e.len as i64).sum(), &group[0]),
            )
            .unwrap();
        }
        let mut rows = (
            Vec::<f32>::new(),
            Vec::<i64>::new(),
            Vec::<i64>::new(),
            Vec::<i64>::new(),
            Vec::<f32>::new(),
        );
        let mut t = 0.0;
        for (j, e) in group.iter().enumerate() {
            let i = g * PER_FILE + j;
            vfrom.push(t);
            vto.push(t + (e.len as i64 + e.video_delta as i64) as f64 / FPS as f64);
            t += e.len as f64 / FPS as f64;
            cols[0].push(i as i64);
            cols[1].push(e.len as i64);
            cols[2].push(from);
            cols[3].push(from + e.len as i64 + e.range_to_delta);
            file_idx.push(g as i64);
            rows.0.extend(&e.ts);
            rows.1.extend(0..e.ts.len() as i64);
            rows.2.extend(std::iter::repeat_n(i as i64, e.ts.len()));
            rows.3.extend(from..from + e.ts.len() as i64);
            rows.4.extend(&e.action);
            from += e.len as i64;
        }
        if !group[0].with_data {
            continue;
        }
        let schema = Arc::new(Schema::new(vec![
            Field::new("timestamp", DataType::Float32, false),
            Field::new("frame_index", DataType::Int64, false),
            Field::new("episode_index", DataType::Int64, false),
            Field::new("index", DataType::Int64, false),
            Field::new(
                "action",
                DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
                false,
            ),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Float32Array::from(rows.0)),
                i64s(rows.1),
                i64s(rows.2),
                i64s(rows.3),
                fsl(rows.4),
            ],
        )
        .unwrap();
        let f = fs::File::create(root.join(format!("data/chunk-000/file-{g:03}.parquet"))).unwrap();
        let mut w = ArrowWriter::try_new(f, schema, None).unwrap();
        w.write(&batch).unwrap();
        w.close().unwrap();
    }

    let n = eps.len();
    let zeros = || i64s(vec![0; n]);
    let names = [
        "episode_index",
        "length",
        "dataset_from_index",
        "dataset_to_index",
        "data/chunk_index",
        "data/file_index",
        "videos/observation.images.top/chunk_index",
        "videos/observation.images.top/file_index",
        "videos/observation.images.top/from_timestamp",
        "videos/observation.images.top/to_timestamp",
        "meta/episodes/chunk_index",
        "meta/episodes/file_index",
    ];
    let arrays: Vec<ArrayRef> = vec![
        i64s(cols[0].clone()),
        i64s(cols[1].clone()),
        i64s(cols[2].clone()),
        i64s(cols[3].clone()),
        zeros(),
        i64s(file_idx.clone()),
        zeros(),
        i64s(file_idx),
        Arc::new(Float64Array::from(vfrom)),
        Arc::new(Float64Array::from(vto)),
        zeros(),
        zeros(),
    ];
    let fields: Vec<Field> = names
        .iter()
        .zip(&arrays)
        .map(|(n, a)| Field::new(*n, a.data_type().clone(), false))
        .collect();
    let _ = StringArray::from(vec!["unused"; 0]);
    let schema = Arc::new(Schema::new(fields));
    let batch = RecordBatch::try_new(schema.clone(), arrays).unwrap();
    let f = fs::File::create(root.join("meta/episodes/chunk-000/file-000.parquet")).unwrap();
    let mut w = ArrowWriter::try_new(f, schema, None).unwrap();
    w.write(&batch).unwrap();
    w.close().unwrap();
}

fn run_v3(eps: Vec<Ep>) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds_v3(dir.path(), &eps);
    validate(&Dataset::open(dir.path()).unwrap()).unwrap()
}

#[test]
fn v3_healthy_packed_dataset_has_no_issues() {
    let v = run_v3(vec![good(20), good(15), good(10), good(12), good(8)]);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn v3_flags_timestamp_drift_in_second_episode_of_a_file() {
    let mut e = good(20);
    for t in e.ts.iter_mut().skip(5) {
        *t += 0.05;
    }
    let v = run_v3(vec![good(10), e, good(10)]);
    assert_eq!(codes(&v), ["timestamp_drift"]);
    assert_eq!(v[0].episode, Some(1));
}

#[test]
fn v3_flags_range_not_matching_length() {
    let mut e = good(10);
    e.range_to_delta = 1;
    let c = codes(&run_v3(vec![good(10), e, good(10)]));
    assert!(c.contains(&"range_length"), "{c:?}");
}

#[test]
fn v3_flags_missing_data_and_video_files() {
    let mut a = good(10);
    a.with_data = false;
    let mut b = good(10);
    b.with_video = false;
    // group 0 = [a, good], group 1 = [b, good]
    let c = codes(&run_v3(vec![a, good(10), b, good(10)]));
    assert!(
        c.contains(&"data_missing") && c.contains(&"video_missing"),
        "{c:?}"
    );
}

// ---------------------------------------------------------------- video checks

#[test]
fn flags_video_frame_count_mismatch() {
    let mut e = good(20);
    e.video_delta = -3;
    let v = run(vec![e], None);
    assert_eq!(codes(&v), ["video_frame_count"]);
    assert_eq!(v[0].severity, Severity::Error);
}

#[test]
fn warns_on_video_fps_mismatch() {
    let mut e = good(20);
    e.video_fps = 15; // same frame count, different timing
    let v = run(vec![e], None);
    assert_eq!(codes(&v), ["video_fps"]);
    assert_eq!(v[0].severity, Severity::Warning);
}

#[test]
fn flags_unreadable_video() {
    let mut e = good(20);
    e.video_garbage = true;
    let v = run(vec![e], None);
    assert_eq!(codes(&v), ["video_unreadable"]);
}

#[test]
fn v3_flags_span_not_matching_length_and_beyond_end() {
    let mut e = good(10);
    e.video_delta = 5; // span claims 5 frames more than the episode has
    let c = codes(&run_v3(vec![good(10), e, good(10)]));
    assert!(c.contains(&"video_span_length"), "{c:?}");
    assert!(c.contains(&"video_span_beyond_end"), "{c:?}"); // episode 1 is last in its file
}

#[test]
fn v3_reports_a_shared_unreadable_video_once() {
    let mut a = good(10);
    a.video_garbage = true; // group 0 = [a, good]: two episodes share one broken file
    let v = run_v3(vec![a, good(10), good(10)]);
    assert_eq!(codes(&v), ["video_unreadable"]);
}

// ---------------------------------------------------------------- malformed input must not panic

/// Replace episode 0's data file of a one-episode v2 dataset with `batch`.
fn run_with_data_file(batch: RecordBatch) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(10)], None);
    let f = fs::File::create(dir.path().join("data/chunk-000/episode_000000.parquet")).unwrap();
    let mut w = ArrowWriter::try_new(f, batch.schema(), None).unwrap();
    w.write(&batch).unwrap();
    w.close().unwrap();
    validate(&Dataset::open(dir.path()).unwrap()).unwrap()
}

#[test]
fn reports_a_missing_timestamp_column_instead_of_panicking() {
    let schema = Arc::new(Schema::new(vec![
        Field::new("frame_index", DataType::Int64, false),
        Field::new(
            "action",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
            false,
        ),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![i64s((0..10).collect()), fsl(vec![0.5; 10 * DIM as usize])],
    )
    .unwrap();
    let c = codes(&run_with_data_file(batch));
    assert!(c.contains(&"column_missing"), "{c:?}");
}

#[test]
fn reports_ragged_list_columns_instead_of_panicking() {
    use arrow_array::{ListArray, types::Float32Type};
    let rows = (0..10).map(|i| Some(vec![Some(i as f32); if i == 0 { 2 } else { 1 }]));
    let action: ArrayRef = Arc::new(ListArray::from_iter_primitive::<Float32Type, _, _>(rows));
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Float32, false),
        Field::new("action", action.data_type().clone(), true),
    ]));
    let ts: Vec<f32> = (0..10).map(|i| i as f32 / FPS).collect();
    let batch =
        RecordBatch::try_new(schema, vec![Arc::new(Float32Array::from(ts)), action]).unwrap();
    let c = codes(&run_with_data_file(batch));
    assert!(c.contains(&"column_shape"), "{c:?}");
}

#[test]
fn rejects_non_positive_fps() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("meta")).unwrap();
    fs::write(
        dir.path().join("meta/info.json"),
        r#"{"codebase_version":"v2.1","fps":0,"total_episodes":0,"total_frames":0,"data_path":"x","features":{}}"#,
    )
    .unwrap();
    let err = Dataset::open(dir.path()).err().unwrap().to_string();
    assert!(err.contains("fps"), "{err}");
}
