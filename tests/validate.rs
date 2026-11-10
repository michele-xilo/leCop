//! Smoke tests on synthetic LeRobot v2.1-style datasets written to a temp dir.
mod common;
use arrow_array::{ArrayRef, FixedSizeListArray, Float32Array, Int64Array, RecordBatch};
use arrow_schema::{DataType, Field, Schema};
use common::Mp4;
use lecop::{
    dataset::Dataset,
    validate::{Issue, Limits, Severity, validate, validate_with, validate_with_limits},
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
    /// Width and height in the video's sample description; info.json declares 4x4.
    video_size: (u16, u16),
    /// v3: this episode's frames are spaced at half the rate in the shared video (dropped frames).
    video_sparse: bool,
    /// The `task_index` of every frame.
    task: i64,
}

/// One video per data file: constant rate, except episodes marked `video_sparse` whose frames are twice as far apart.
fn v3_video_bytes(group: &[Ep]) -> Vec<u8> {
    if group[0].video_garbage {
        return video_bytes(0, &group[0]);
    }
    let mut m = Mp4::constant_fps(0, group[0].video_fps);
    m.size = Some(group[0].video_size);
    m.stts = group
        .iter()
        .map(|e| (e.len as u32, if e.video_sparse { 2048 } else { 1024 }))
        .collect();
    m.bytes()
}

fn video_bytes(frames: i64, e: &Ep) -> Vec<u8> {
    if e.video_garbage {
        return b"not an mp4".to_vec();
    }
    let mut m = Mp4::constant_fps(frames.max(0) as u32, e.video_fps);
    m.size = Some(e.video_size);
    m.bytes()
}

/// A healthy episode: perfect timestamps, varying action. Every call gets its own action values, so two
/// episodes are never duplicates unless a test copies one.
fn good(len: usize) -> Ep {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let base = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as f32 * 1000.0;
    Ep {
        len,
        ts: (0..len).map(|i| i as f32 / FPS).collect(),
        // Steps that vary a little: a perfectly linear action has a step deviation of float noise, and every
        // ordinary step would then look like a jump.
        action: (0..len * DIM as usize)
            .map(|i| {
                let row = i / DIM as usize;
                base + i as f32 * 0.1 + ((row * 7) % 5) as f32 * 0.05
            })
            .collect(),
        with_video: true,
        with_data: true,
        range_to_delta: 0,
        video_delta: 0,
        video_fps: FPS as u32,
        video_garbage: false,
        video_size: (4, 4),
        video_sparse: false,
        task: 0,
    }
}

/// A steadily rising `observation.state` that no check has anything to say about.
fn ramp(frames: usize) -> Vec<f32> {
    (0..frames * DIM as usize)
        .map(|i| 1.0 + i as f32 * 0.5)
        .collect()
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
            "observation.state": {"dtype": "float32", "shape": [DIM]},
            "observation.images.top": {"dtype": "video", "shape": [4, 4, 3]},
            "timestamp": {"dtype": "float32", "shape": [1]},
            "frame_index": {"dtype": "int64", "shape": [1]},
            "task_index": {"dtype": "int64", "shape": [1]},
        }
    });
    fs::write(root.join("meta/info.json"), info.to_string()).unwrap();
    common::write_stats(root);
    fs::write(
        root.join("meta/tasks.jsonl"),
        "{\"task_index\": 0, \"task\": \"t\"}\n",
    )
    .unwrap();
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
            Field::new("task_index", DataType::Int64, false),
            Field::new(
                "action",
                DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
                false,
            ),
            Field::new(
                "observation.state",
                DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
                false,
            ),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Float32Array::from(e.ts.clone())),
                Arc::new(Int64Array::from((0..e.ts.len() as i64).collect::<Vec<_>>())),
                i64s(vec![e.task; e.len]),
                fsl(e.action.clone()),
                fsl(ramp(e.len)),
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
            "observation.state": {"dtype": "float32", "shape": [DIM]},
            "observation.images.top": {"dtype": "video", "shape": [4, 4, 3]},
            "timestamp": {"dtype": "float32", "shape": [1]},
            "frame_index": {"dtype": "int64", "shape": [1]},
            "episode_index": {"dtype": "int64", "shape": [1]},
            "index": {"dtype": "int64", "shape": [1]},
        }
    });
    fs::write(root.join("meta/info.json"), info.to_string()).unwrap();
    common::write_stats(root);

    let (mut from, mut cols): (i64, [Vec<i64>; 4]) = (0, Default::default()); // ep, len, from, to
    let mut file_idx = Vec::new();
    let (mut vfrom, mut vto) = (Vec::<f64>::new(), Vec::<f64>::new());
    for (g, group) in eps.chunks(PER_FILE).enumerate() {
        if group[0].with_video {
            fs::write(
                root.join(format!("videos/{vk}/chunk-000/file-{g:03}.mp4")),
                v3_video_bytes(group),
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
            Field::new(
                "observation.state",
                DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
                false,
            ),
        ]));
        let frames = rows.0.len();
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Float32Array::from(rows.0)),
                i64s(rows.1),
                i64s(rows.2),
                i64s(rows.3),
                fsl(rows.4),
                fsl(ramp(frames)),
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
    let v = run_v3(vec![good(20), good(15), good(10), good(12), good(18)]);
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
fn v3_flags_episodes_whose_span_holds_the_wrong_number_of_frames() {
    let mut sparse = good(10);
    sparse.video_sparse = true; // its frames are 0.2 s apart, so the 1 s span holds 5 of them
    let v = run_v3(vec![good(10), sparse]); // both in one shared video
    let bad: Vec<_> = v
        .iter()
        .filter(|i| i.code == "video_frame_count")
        .map(|i| i.episode)
        .collect();
    assert_eq!(bad, [Some(1)], "{v:?}");
}

#[test]
fn v3_healthy_packed_dataset_has_no_frame_count_findings() {
    let v = run_v3(vec![good(10), good(12), good(17)]);
    assert!(v.is_empty(), "{v:?}");
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

// ---------------------------------------------------------------- semantic checks

fn edit_info(root: &Path, f: impl FnOnce(&mut serde_json::Value)) {
    let p = root.join("meta/info.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
    f(&mut v);
    fs::write(p, v.to_string()).unwrap();
    common::write_stats(root);
}

/// Rename a feature in info.json and its column in every data file, as a real dataset would have it.
fn rename_feature(root: &Path, from: &str, to: &str) {
    edit_info(root, |v| {
        let f = v["features"].as_object_mut().unwrap();
        let spec = f.remove(from).unwrap();
        f.insert(to.into(), spec);
    });
    for chunk in fs::read_dir(root.join("data")).unwrap() {
        for file in fs::read_dir(chunk.unwrap().path()).unwrap() {
            let path = file.unwrap().path();
            let reader = parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder::try_new(
                fs::File::open(&path).unwrap(),
            )
            .unwrap()
            .build()
            .unwrap();
            let batches: Vec<RecordBatch> = reader.map(|b| b.unwrap()).collect();
            let fields: Vec<Field> = batches[0]
                .schema()
                .fields()
                .iter()
                .map(|f| match f.name() == from {
                    true => f.as_ref().clone().with_name(to),
                    false => f.as_ref().clone(),
                })
                .collect();
            let schema = Arc::new(Schema::new(fields));
            let mut w =
                ArrowWriter::try_new(fs::File::create(&path).unwrap(), schema.clone(), None)
                    .unwrap();
            for b in batches {
                w.write(&RecordBatch::try_new(schema.clone(), b.columns().to_vec()).unwrap())
                    .unwrap();
            }
            w.close().unwrap();
        }
    }
}

fn rename_action_feature(root: &Path) {
    edit_info(root, |v| {
        let f = v["features"].as_object_mut().unwrap();
        let a = f.remove("action").unwrap();
        f.insert("actions".into(), a);
    });
}

#[test]
fn warns_when_there_is_no_standard_action_feature() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    rename_feature(dir.path(), "action", "actions");
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["nonstandard_features"]);
    assert_eq!(v[0].severity, Severity::Warning);
    assert_eq!(v[0].episode, None);
}

#[test]
fn action_checks_follow_an_actions_column() {
    // piperx-style dataset: the column is `actions`, so the frozen-action check must still run.
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(10)], None);
    rename_action_feature(dir.path());
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Float32, false),
        Field::new(
            "actions",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), DIM),
            false,
        ),
    ]));
    let ts: Vec<f32> = (0..10).map(|i| i as f32 / FPS).collect();
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(Float32Array::from(ts)),
            fsl(vec![0.5; 10 * DIM as usize]),
        ],
    )
    .unwrap();
    let f = fs::File::create(dir.path().join("data/chunk-000/episode_000000.parquet")).unwrap();
    let mut w = ArrowWriter::try_new(f, batch.schema(), None).unwrap();
    w.write(&batch).unwrap();
    w.close().unwrap();
    let c = codes(&validate(&Dataset::open(dir.path()).unwrap()).unwrap());
    assert!(c.contains(&"action_constant"), "{c:?}");
}

/// `idle_rows` leading rows repeat the same action, then the action changes every frame.
fn idle_then_moving(len: usize, idle_rows: usize) -> Ep {
    let mut e = good(len);
    e.action = (0..len)
        .flat_map(|i| {
            let v = if i < idle_rows { 0.5 } else { i as f32 * 0.1 };
            [v, v]
        })
        .collect();
    e
}

#[test]
fn warns_on_mostly_idle_episode_and_says_when_it_starts_moving() {
    let v = run(vec![idle_then_moving(20, 15)], None); // 14 of 19 transitions repeat
    assert_eq!(codes(&v), ["idle_episode"]);
    assert_eq!(v[0].severity, Severity::Warning);
    assert!(v[0].message.contains("frame 15"), "{}", v[0].message);
}

#[test]
fn does_not_warn_on_a_short_pause() {
    let v = run(vec![idle_then_moving(20, 6)], None); // 5 of 19 transitions repeat
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn does_not_mistake_a_sibling_feature_for_the_action() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    // a float feature that only shares the prefix
    rename_feature(dir.path(), "action", "action_mask");
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["nonstandard_features"]);
    assert!(!v[0].message.contains("action_mask"), "{}", v[0].message);
}

#[test]
fn report_is_identical_for_any_thread_count() {
    use common::synth::{Synth, write_v2, write_v3};
    let s = Synth {
        episodes: 40,
        frames: 50,
        dim: 3,
        fps: 30,
        per_file: 7,
    };
    for v3 in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        if v3 {
            write_v3(dir.path(), &s);
        } else {
            write_v2(dir.path(), &s);
        }
        // Wrong fps and frame total: findings in every episode, every phase, in many files.
        edit_info(dir.path(), |v| {
            v["fps"] = 31.into();
            v["total_frames"] = 1.into();
        });
        let ds = Dataset::open(dir.path()).unwrap();
        let one = format!("{:?}", validate_with(&ds, 1).unwrap());
        assert!(
            one.contains("timestamp_drift"),
            "fixture must have findings"
        );
        for jobs in [2, 8] {
            assert_eq!(
                one,
                format!("{:?}", validate_with(&ds, jobs).unwrap()),
                "v3={v3} jobs={jobs}"
            );
        }
    }
}

// ---------------------------------------------------------------- feature schema vs info.json

fn schema_issues_after(edit: impl FnOnce(&mut serde_json::Value)) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20), good(15)], None);
    edit_info(dir.path(), edit);
    validate(&Dataset::open(dir.path()).unwrap()).unwrap()
}

#[test]
fn flags_a_declared_feature_missing_from_the_data_once() {
    let v = schema_issues_after(|v| {
        v["features"]["observation.extra"] = serde_json::json!({"dtype": "float32", "shape": [3]});
    });
    assert_eq!(codes(&v), ["feature_missing"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Error);
    assert!(v[0].message.contains("observation.extra"), "{v:?}");
}

#[test]
fn flags_a_column_whose_dtype_differs_from_info_json() {
    let v = schema_issues_after(|v| {
        v["features"]["action"]["dtype"] = "int64".into();
    });
    assert_eq!(codes(&v), ["feature_dtype"], "{v:?}");
}

#[test]
fn flags_a_column_whose_width_differs_from_info_json() {
    let v = schema_issues_after(|v| {
        v["features"]["action"]["shape"] = serde_json::json!([DIM + 1]);
    });
    assert_eq!(codes(&v), ["feature_shape"], "{v:?}");
}

#[test]
fn warns_on_a_data_column_info_json_does_not_declare() {
    let v = schema_issues_after(|v| {
        v["features"].as_object_mut().unwrap().remove("frame_index");
    });
    assert_eq!(codes(&v), ["feature_extra"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Warning);
}

// ---------------------------------------------------------------- episode shape

#[test]
fn warns_on_an_episode_shorter_than_a_second() {
    let v = run(vec![good(20), good(3)], None);
    let short: Vec<_> = v.iter().filter(|i| i.code == "episode_short").collect();
    assert_eq!(short.len(), 1, "{v:?}");
    assert_eq!(short[0].episode, Some(1));
    assert_eq!(short[0].severity, Severity::Warning);
}

#[test]
fn flags_an_empty_episode() {
    let v = run(vec![good(20), good(0)], None);
    assert!(codes(&v).contains(&"episode_empty"), "{v:?}");
    assert!(!codes(&v).contains(&"episode_short"), "{v:?}");
}

#[test]
fn warns_on_an_episode_far_longer_than_the_others() {
    let mut eps: Vec<Ep> = (0..20).map(|i| good(20 + i % 4)).collect();
    eps.push(good(400));
    let v = run(eps, None);
    let out: Vec<_> = v
        .iter()
        .filter(|i| i.code == "episode_length_outlier")
        .collect();
    assert_eq!(out.len(), 1, "{v:?}");
    assert_eq!(out[0].episode, Some(20));
    assert_eq!(out[0].severity, Severity::Warning);
}

#[test]
fn does_not_call_a_few_episodes_outliers() {
    let v = run(vec![good(20), good(21), good(400)], None);
    assert!(!codes(&v).contains(&"episode_length_outlier"), "{v:?}");
}

#[test]
fn warns_on_an_episode_that_repeats_an_earlier_one() {
    let first = good(20);
    let mut copy = good(20);
    copy.action = first.action.clone();
    let v = run(vec![first, good(15), copy], None);
    let dup: Vec<_> = v.iter().filter(|i| i.code == "duplicate_episode").collect();
    assert_eq!(dup.len(), 1, "{v:?}");
    assert_eq!(dup[0].episode, Some(2));
    assert!(dup[0].message.contains("episode 0"), "{}", dup[0].message);
    assert_eq!(dup[0].severity, Severity::Warning);
}

#[test]
fn does_not_flag_same_length_episodes_with_different_actions() {
    let first = good(20);
    let mut other = good(20);
    other.action = first.action.clone();
    other.action[5] += 1.0;
    let v = run(vec![first, other], None);
    assert!(!codes(&v).contains(&"duplicate_episode"), "{v:?}");
}

// ---------------------------------------------------------------- numeric anomalies

fn action_column_run(f: impl Fn(usize, usize) -> f32) -> Vec<Issue> {
    let mut e = good(150);
    e.action = (0..150)
        .flat_map(|row| (0..DIM as usize).map(move |d| (row, d)))
        .map(|(row, d)| f(row, d))
        .collect();
    run(vec![e], None)
}

#[test]
fn a_varying_action_has_no_numeric_findings() {
    let v = action_column_run(|row, d| row as f32 * 0.01 + d as f32);
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn warns_on_a_dimension_that_never_changes() {
    let v = action_column_run(|row, d| if d == 0 { row as f32 * 0.01 } else { 0.5 });
    let f: Vec<_> = v
        .iter()
        .filter(|i| i.code == "constant_dimension")
        .collect();
    assert_eq!(f.len(), 1, "{v:?}");
    assert_eq!(f[0].severity, Severity::Warning);
    assert!(
        f[0].message.contains("action") && f[0].message.contains('1'),
        "{}",
        f[0].message
    );
}

#[test]
fn warns_on_a_dimension_stuck_at_its_maximum() {
    let v = action_column_run(|row, d| match (d, row) {
        (0, _) => row as f32 * 0.01,
        (_, 3) => 4.0,
        _ => 5.0,
    });
    assert!(codes(&v).contains(&"clipped_dimension"), "{v:?}");
}

#[test]
fn warns_on_a_value_far_from_the_rest() {
    let v = action_column_run(|row, d| match (d, row) {
        (1, 70) => 1000.0,
        _ => row as f32 * 0.01 + d as f32,
    });
    let f: Vec<_> = v.iter().filter(|i| i.code == "outlier_values").collect();
    assert_eq!(f.len(), 1, "{v:?}");
}

// ---------------------------------------------------------------- video resolution

#[test]
fn flags_a_video_whose_size_differs_from_info_json_once() {
    let mut a = good(20);
    a.video_size = (8, 6);
    let mut b = good(15);
    b.video_size = (8, 6);
    let v = run(vec![a, b, good(12)], None);
    let r: Vec<_> = v.iter().filter(|i| i.code == "video_resolution").collect();
    assert_eq!(r.len(), 1, "{v:?}");
    assert!(
        r[0].message.contains("8x6") && r[0].message.contains("4x4"),
        "{}",
        r[0].message
    );
}

#[test]
fn flags_the_same_in_a_packed_v3_dataset() {
    let mut a = good(20);
    a.video_size = (8, 6);
    let v = run_v3(vec![a, good(15)]);
    assert_eq!(codes(&v), ["video_resolution"], "{v:?}");
}

#[test]
fn reads_channel_first_shapes_from_the_feature_names() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    edit_info(dir.path(), |v| {
        let f = &mut v["features"]["observation.images.top"];
        f["shape"] = serde_json::json!([3, 4, 4]);
        f["names"] = serde_json::json!(["channels", "height", "width"]);
    });
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert!(v.is_empty(), "{v:?}");
}

fn write_tasks_parquet(root: &Path, n: usize) {
    let schema = Arc::new(Schema::new(vec![Field::new(
        "task_index",
        DataType::Int64,
        false,
    )]));
    let col: ArrayRef = i64s((0..n as i64).collect());
    let batch = RecordBatch::try_new(schema.clone(), vec![col]).unwrap();
    let f = fs::File::create(root.join("meta/tasks.parquet")).unwrap();
    let mut w = ArrowWriter::try_new(f, schema, None).unwrap();
    w.write(&batch).unwrap();
    w.close().unwrap();
}

fn run_with_tasks(declared: usize, jsonl_lines: Option<usize>) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    edit_info(dir.path(), |v| v["total_tasks"] = declared.into());
    if let Some(n) = jsonl_lines {
        let lines: Vec<String> = (0..n)
            .map(|i| format!("{{\"task_index\": {i}, \"task\": \"t{i}\"}}"))
            .collect();
        fs::write(dir.path().join("meta/tasks.jsonl"), lines.join("\n")).unwrap();
    } else {
        fs::remove_file(dir.path().join("meta/tasks.jsonl")).unwrap();
    }
    validate(&Dataset::open(dir.path()).unwrap()).unwrap()
}

#[test]
fn flags_declared_tasks_without_task_metadata() {
    let v = run_with_tasks(1, None);
    assert_eq!(codes(&v), ["tasks_missing"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Error);
}

#[test]
fn warns_when_total_tasks_disagrees_with_the_task_file() {
    let v = run_with_tasks(1, Some(2));
    assert_eq!(codes(&v), ["tasks_count"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Warning);
}

#[test]
fn matching_task_file_is_clean() {
    let v = run_with_tasks(2, Some(2));
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn v3_reads_the_task_count_from_tasks_parquet() {
    for (rows, want) in [(1, vec![]), (3, vec!["tasks_count"])] {
        let dir = tempfile::tempdir().unwrap();
        write_ds_v3(dir.path(), &[good(20)]);
        edit_info(dir.path(), |v| v["total_tasks"] = 1.into());
        write_tasks_parquet(dir.path(), rows);
        let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
        assert_eq!(codes(&v), want, "{v:?}");
    }
    let dir = tempfile::tempdir().unwrap();
    write_ds_v3(dir.path(), &[good(20)]);
    edit_info(dir.path(), |v| v["total_tasks"] = 1.into());
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["tasks_missing"], "{v:?}");
}

fn stats_path(root: &Path) -> std::path::PathBuf {
    root.join("meta/episodes_stats.jsonl")
}

#[test]
fn warns_when_the_statistics_file_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    fs::remove_file(stats_path(dir.path())).unwrap();
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["stats_missing"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Warning);
}

#[test]
fn warns_when_a_feature_has_no_statistics() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    fs::write(
        stats_path(dir.path()),
        "{\"episode_index\":0,\"stats\":{\"action\":{}}}\n",
    )
    .unwrap();
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["stats_features"], "{v:?}");
    assert!(v[0].message.contains("timestamp"), "{}", v[0].message);
    assert!(!v[0].message.contains("action"), "{}", v[0].message);
}

#[test]
fn v3_reads_stats_json() {
    let dir = tempfile::tempdir().unwrap();
    write_ds_v3(dir.path(), &[good(20)]);
    let p = dir.path().join("meta/stats.json");
    fs::write(&p, r#"{"action": {}}"#).unwrap();
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["stats_features"], "{v:?}");
    fs::write(&p, "not json").unwrap();
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["stats_missing"], "{v:?}");
}

#[test]
fn flags_a_task_index_missing_from_the_task_metadata() {
    let mut a = good(20);
    a.task = 3;
    let mut b = good(15);
    b.task = 7;
    let v = run(vec![good(12), a, b], None);
    assert_eq!(codes(&v), ["task_index"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Error);
    assert!(v[0].message.contains("3, 7"), "{}", v[0].message);
}

#[test]
fn warns_when_there_is_no_observation_state() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    rename_feature(dir.path(), "observation.state", "observation.joints");
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["nonstandard_features"], "{v:?}");
    assert!(
        v[0].message.contains("observation.state"),
        "{}",
        v[0].message
    );
}

#[test]
fn names_every_missing_info_field_at_once() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    edit_info(dir.path(), |v| {
        let o = v.as_object_mut().unwrap();
        o.remove("fps");
        o.remove("total_frames");
    });
    let err = Dataset::open(dir.path()).err().unwrap().to_string();
    assert!(
        err.contains("missing required fields: fps, total_frames"),
        "{err}"
    );
}

#[test]
fn v2_0_datasets_use_stats_json_and_are_otherwise_checked_like_v2_1() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20), good(15)], None);
    fs::remove_file(stats_path(dir.path())).unwrap();
    edit_info(dir.path(), |v| v["codebase_version"] = "v2.0".into());
    assert!(dir.path().join("meta/stats.json").exists());
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert!(v.is_empty(), "{v:?}");
    fs::remove_file(dir.path().join("meta/stats.json")).unwrap();
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["stats_missing"], "{v:?}");
}

#[test]
fn flags_an_episode_whose_action_jumps_and_names_it() {
    let mut eps: Vec<Ep> = (0..8).map(|_| good(20)).collect();
    eps[3].action[2 * DIM as usize] += 1000.0;
    eps[3].action[2 * DIM as usize + 1] += 1000.0;
    let v = run(eps, None);
    let j: Vec<_> = v.iter().filter(|i| i.code == "action_jump").collect();
    assert_eq!(j.len(), 1, "{v:?}");
    assert_eq!((j[0].episode, j[0].severity), (Some(3), Severity::Warning));
    assert!(j[0].message.starts_with("2 "), "{}", j[0].message);
}

#[test]
fn flags_action_jumps_in_a_packed_v3_dataset() {
    let mut eps: Vec<Ep> = (0..8).map(|_| good(20)).collect();
    eps[6].action[5 * DIM as usize] += 1000.0;
    eps[6].action[5 * DIM as usize + 1] += 1000.0;
    let v = run_v3(eps);
    let j: Vec<_> = v.iter().filter(|i| i.code == "action_jump").collect();
    assert_eq!(j.len(), 1, "{v:?}");
    assert_eq!(j[0].episode, Some(6));
}

#[test]
fn lists_at_most_five_jumping_episodes_and_counts_the_rest() {
    // Jumps inflate the step deviation they are judged by, so there are many ordinary episodes among the seven.
    let mut eps: Vec<Ep> = (0..40).map(|_| good(20)).collect();
    for e in eps.iter_mut().take(7) {
        for a in e.action.iter_mut().skip(2 * DIM as usize) {
            *a += 1000.0;
        }
    }
    let v = run(eps, None);
    let j: Vec<_> = v.iter().filter(|i| i.code == "action_jump").collect();
    assert_eq!(j.len(), 6, "{v:?}");
    let summary = j.iter().find(|i| i.episode.is_none()).unwrap();
    assert!(summary.message.contains("2 more"), "{}", summary.message);
}

fn drifting(n: usize, shift: f32) -> Vec<Ep> {
    (0..n)
        .map(|i| {
            let mut e = good(20);
            let off = if i >= n - n / 4 { shift } else { 0.0 };
            // The same action in every episode, so only the offset can move the mean.
            e.action = (0..20 * DIM as usize)
                .map(|k| k as f32 * 0.1 + off)
                .collect();
            e.action
                .iter_mut()
                .enumerate()
                .for_each(|(k, a)| *a += (i % 2) as f32 * 0.01 * k as f32);
            e
        })
        .collect()
}

#[test]
fn flags_a_shift_between_the_first_and_last_quarter_of_episodes() {
    let v = run(drifting(12, 50.0), None);
    let s: Vec<_> = v
        .iter()
        .filter(|i| i.code == "distribution_shift")
        .collect();
    assert_eq!(s.len(), 1, "{v:?}");
    assert!(s[0].message.starts_with("action: "), "{}", s[0].message);
}

#[test]
fn no_shift_without_a_drift_and_none_under_ten_episodes() {
    let v = run(drifting(12, 0.0), None);
    assert!(v.iter().all(|i| i.code != "distribution_shift"), "{v:?}");
    let v = run(drifting(9, 50.0), None);
    assert!(v.iter().all(|i| i.code != "distribution_shift"), "{v:?}");
}

#[test]
fn warns_on_an_absolute_data_path_template() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    // The files still resolve, because joining an absolute template replaces the dataset root.
    let abs = format!(
        "{}/data/chunk-{{episode_chunk:03d}}/episode_{{episode_index:06d}}.parquet",
        dir.path().display()
    );
    edit_info(dir.path(), |v| v["data_path"] = abs.into());
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["path_absolute"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Warning);
}

#[test]
fn warns_on_non_parquet_files_in_the_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    fs::write(dir.path().join("data/chunk-000/.DS_Store"), b"x").unwrap();
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["data_stray_file"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Warning);
    assert!(v[0].message.contains(".DS_Store"), "{v:?}");
}

#[test]
fn lists_five_stray_files_and_counts_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    for i in 0..7 {
        fs::write(dir.path().join(format!("data/chunk-000/x{i}.tmp")), b"x").unwrap();
    }
    let v = validate(&Dataset::open(dir.path()).unwrap()).unwrap();
    assert_eq!(codes(&v), ["data_stray_file"], "{v:?}");
    assert!(v[0].message.ends_with("and 2 more"), "{v:?}");
}

fn run_limits(limits: serde_json::Value) -> Vec<Issue> {
    let dir = tempfile::tempdir().unwrap();
    write_ds(dir.path(), &[good(20)], None);
    let limits: Limits = serde_json::from_value(serde_json::json!({ "action": limits })).unwrap();
    validate_with_limits(&Dataset::open(dir.path()).unwrap(), 1, &limits).unwrap()
}

#[test]
fn limits_that_hold_report_nothing() {
    let v = run_limits(serde_json::json!({"min": -1e9, "max": 1e9, "max_step": 100.0}));
    assert!(v.is_empty(), "{v:?}");
}

#[test]
fn flags_a_value_beyond_a_scalar_bound() {
    let v = run_limits(serde_json::json!({"max": -1e9}));
    assert_eq!(codes(&v), ["out_of_range"], "{v:?}");
    assert_eq!(v[0].severity, Severity::Error);
    assert!(v[0].message.starts_with("action: dimension 0"), "{v:?}");
}

#[test]
fn per_dimension_bounds_flag_only_the_dimension_that_breaks_them() {
    let v = run_limits(serde_json::json!({"min": [1e9, -1e9]}));
    assert_eq!(codes(&v), ["out_of_range"], "{v:?}");
    assert!(
        v[0].message.contains("dimension 0") && !v[0].message.contains("dimension 1"),
        "{v:?}"
    );
}

#[test]
fn flags_a_bound_list_of_the_wrong_width() {
    let v = run_limits(serde_json::json!({"min": [0.0, 0.0, 0.0]}));
    assert_eq!(codes(&v), ["limits_width"], "{v:?}");
}

#[test]
fn flags_the_episode_whose_frame_to_frame_change_exceeds_max_step() {
    let v = run_limits(serde_json::json!({"max_step": 0.001}));
    assert_eq!(codes(&v), ["step_limit"], "{v:?}");
    assert_eq!(v[0].episode, Some(0));
}
