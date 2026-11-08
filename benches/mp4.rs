//! How fast is reading frame count and duration from an mp4's metadata?
//! Synthetic files, moov last (like ffmpeg output) after a zero-filled mdat. The stsz table has one
//! entry per frame, so the moov grows with the frame count while the mdat size varies independently.
use criterion::{Criterion, criterion_group, criterion_main};
use lecop::mp4::read_video_info;
use std::hint::black_box;

#[path = "../tests/common/mod.rs"]
mod common;

fn bench(c: &mut Criterion) {
    let dir = std::env::temp_dir().join("lecop-bench-mp4");
    std::fs::create_dir_all(&dir).unwrap();
    let mut g = c.benchmark_group("read_video_info");
    // (name, frames, mdat bytes): one pusht episode, the packed v3 pusht video, a long shard
    let cases = [
        ("161_frames_small_mdat", 161u32, 1 << 10),
        ("161_frames_64MiB_mdat", 161, 64 << 20),
        ("25650_frames_64MiB_mdat", 25_650, 64 << 20),
        ("500000_frames_64MiB_mdat", 500_000, 64 << 20),
    ];
    for (name, frames, mdat_len) in cases {
        let mut m = common::Mp4::constant_fps(frames, 10);
        m.mdat_len = mdat_len;
        let path = dir.join(format!("{name}.mp4"));
        std::fs::write(&path, m.bytes()).unwrap();
        g.bench_function(name, |b| {
            b.iter(|| read_video_info(black_box(&path)).unwrap())
        });
    }
    g.finish();
    let _ = std::fs::remove_dir_all(&dir);
}

criterion_group!(benches, bench);
criterion_main!(benches);
