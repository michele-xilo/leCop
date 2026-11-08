//! Large synthetic LeRobot datasets for benchmarks. Healthy by construction (validate reports nothing),
//! deterministic (fixed seed), written once per run into a temp dir.
#![allow(dead_code)]
use super::Mp4;
use arrow_array::{
    ArrayRef, FixedSizeListArray, Float32Array, Float64Array, Int64Array, RecordBatch,
};
use arrow_schema::{DataType, Field, Schema};
use parquet::arrow::ArrowWriter;
use std::{fs, path::Path, sync::Arc};

pub struct Synth {
    pub episodes: usize,
    pub frames: usize,
    pub dim: usize,
    pub fps: u32,
    /// v3 only: episodes packed per data/video file.
    pub per_file: usize,
}

const VK: &str = "observation.images.top";

/// xorshift64: reproducible without a dependency.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn i64s(v: Vec<i64>) -> ArrayRef {
    Arc::new(Int64Array::from(v))
}

fn fsl(vals: Vec<f32>, dim: usize) -> ArrayRef {
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    Arc::new(FixedSizeListArray::new(
        item,
        dim as i32,
        Arc::new(Float32Array::from(vals)),
        None,
    ))
}

fn fsl_field(name: &str, dim: usize) -> Field {
    let item = Arc::new(Field::new("item", DataType::Float32, true));
    Field::new(name, DataType::FixedSizeList(item, dim as i32), false)
}

fn write_parquet(path: &Path, schema: Arc<Schema>, cols: Vec<ArrayRef>) {
    let batch = RecordBatch::try_new(schema.clone(), cols).unwrap();
    let mut w = ArrowWriter::try_new(fs::File::create(path).unwrap(), schema, None).unwrap();
    w.write(&batch).unwrap();
    w.close().unwrap();
}

/// Rows for episodes `eps` (global frame numbering starts at `first_index`).
fn data_columns(
    s: &Synth,
    eps: std::ops::Range<usize>,
    first_index: i64,
    rng: &mut Rng,
) -> (Arc<Schema>, Vec<ArrayRef>) {
    let n = eps.len() * s.frames;
    let (mut ts, mut fi, mut ep, mut idx) = (
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
        Vec::with_capacity(n),
    );
    let mut action = Vec::with_capacity(n * s.dim);
    let mut state = Vec::with_capacity(n * s.dim);
    let mut pos = vec![0.0f32; s.dim];
    for e in eps {
        for f in 0..s.frames {
            ts.push(f as f32 / s.fps as f32);
            fi.push(f as i64);
            ep.push(e as i64);
            idx.push(first_index + ep.len() as i64 - 1);
            for p in pos.iter_mut() {
                *p += rng.next() - 0.5; // random walk: the action changes every frame
            }
            action.extend(&pos);
            state.extend(pos.iter().map(|p| p + 0.01));
        }
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new("timestamp", DataType::Float32, false),
        Field::new("frame_index", DataType::Int64, false),
        Field::new("episode_index", DataType::Int64, false),
        Field::new("index", DataType::Int64, false),
        fsl_field("action", s.dim),
        fsl_field("observation.state", s.dim),
    ]));
    let cols = vec![
        Arc::new(Float32Array::from(ts)) as ArrayRef,
        i64s(fi),
        i64s(ep),
        i64s(idx),
        fsl(action, s.dim),
        fsl(state, s.dim),
    ];
    (schema, cols)
}

fn info(s: &Synth, version: &str, data_path: &str, video_path: &str) -> String {
    let vec = |d: usize| serde_json::json!({"dtype": "float32", "shape": [d]});
    serde_json::json!({
        "codebase_version": version, "robot_type": "synthetic", "fps": s.fps,
        "total_episodes": s.episodes, "total_frames": s.episodes * s.frames, "chunks_size": 1000,
        "data_path": data_path, "video_path": video_path,
        "features": {
            "action": vec(s.dim), "observation.state": vec(s.dim),
            VK: {"dtype": "video", "shape": [4, 4, 3]},
            "timestamp": vec(1), "frame_index": {"dtype": "int64", "shape": [1]},
            "episode_index": {"dtype": "int64", "shape": [1]}, "index": {"dtype": "int64", "shape": [1]},
        }
    })
    .to_string()
}

fn mp4(frames: usize, fps: u32) -> Vec<u8> {
    Mp4::constant_fps(frames as u32, fps).bytes()
}

/// One data parquet and one mp4 per episode.
pub fn write_v2(root: &Path, s: &Synth) {
    let chunk = |e: usize| e / 1000;
    for d in ["meta", "data", "videos"] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    fs::write(
        root.join("meta/info.json"),
        info(
            s,
            "v2.1",
            "data/chunk-{episode_chunk:03d}/episode_{episode_index:06d}.parquet",
            "videos/chunk-{episode_chunk:03d}/{video_key}/episode_{episode_index:06d}.mp4",
        ),
    )
    .unwrap();
    let jsonl: String = (0..s.episodes)
        .map(|e| {
            format!(
                "{}\n",
                serde_json::json!({"episode_index": e, "length": s.frames, "tasks": ["t"]})
            )
        })
        .collect();
    fs::write(root.join("meta/episodes.jsonl"), jsonl).unwrap();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    for e in 0..s.episodes {
        let dd = root.join(format!("data/chunk-{:03}", chunk(e)));
        let vd = root.join(format!("videos/chunk-{:03}/{VK}", chunk(e)));
        fs::create_dir_all(&dd).unwrap();
        fs::create_dir_all(&vd).unwrap();
        let (schema, cols) = data_columns(s, e..e + 1, (e * s.frames) as i64, &mut rng);
        write_parquet(&dd.join(format!("episode_{e:06}.parquet")), schema, cols);
        fs::write(vd.join(format!("episode_{e:06}.mp4")), mp4(s.frames, s.fps)).unwrap();
    }
}

/// `per_file` episodes per data parquet and per mp4; episode boundaries in meta/episodes.
pub fn write_v3(root: &Path, s: &Synth) {
    for d in [
        "meta/episodes/chunk-000",
        "data/chunk-000",
        &format!("videos/{VK}/chunk-000"),
    ] {
        fs::create_dir_all(root.join(d)).unwrap();
    }
    fs::write(
        root.join("meta/info.json"),
        info(
            s,
            "v3.0",
            "data/chunk-{chunk_index:03d}/file-{file_index:03d}.parquet",
            "videos/{video_key}/chunk-{chunk_index:03d}/file-{file_index:03d}.mp4",
        ),
    )
    .unwrap();
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let fps = s.fps as f64;
    let (mut ep_col, mut len, mut from, mut to, mut file, mut vfrom, mut vto) =
        (vec![], vec![], vec![], vec![], vec![], vec![], vec![]);
    for (g, first) in (0..s.episodes).step_by(s.per_file).enumerate() {
        let eps = first..(first + s.per_file).min(s.episodes);
        let (schema, cols) = data_columns(s, eps.clone(), (first * s.frames) as i64, &mut rng);
        write_parquet(
            &root.join(format!("data/chunk-000/file-{g:03}.parquet")),
            schema,
            cols,
        );
        fs::write(
            root.join(format!("videos/{VK}/chunk-000/file-{g:03}.mp4")),
            mp4(eps.len() * s.frames, s.fps),
        )
        .unwrap();
        for (j, e) in eps.enumerate() {
            ep_col.push(e as i64);
            len.push(s.frames as i64);
            from.push((e * s.frames) as i64);
            to.push(((e + 1) * s.frames) as i64);
            file.push(g as i64);
            vfrom.push((j * s.frames) as f64 / fps);
            vto.push(((j + 1) * s.frames) as f64 / fps);
        }
    }
    let n = ep_col.len();
    let zeros = || i64s(vec![0; n]);
    let names = [
        "episode_index",
        "length",
        "dataset_from_index",
        "dataset_to_index",
        "data/chunk_index",
        "data/file_index",
        &format!("videos/{VK}/chunk_index"),
        &format!("videos/{VK}/file_index"),
        &format!("videos/{VK}/from_timestamp"),
        &format!("videos/{VK}/to_timestamp"),
        "meta/episodes/chunk_index",
        "meta/episodes/file_index",
    ];
    let arrays: Vec<ArrayRef> = vec![
        i64s(ep_col),
        i64s(len),
        i64s(from),
        i64s(to),
        zeros(),
        i64s(file.clone()),
        zeros(),
        i64s(file),
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
    write_parquet(
        &root.join("meta/episodes/chunk-000/file-000.parquet"),
        Arc::new(Schema::new(fields)),
        arrays,
    );
}
