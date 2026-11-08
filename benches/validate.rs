//! End-to-end `validate` on large synthetic datasets (healthy, fixed seed), plus the real datasets in
//! `datasets/` when present. Generation happens once, outside the timed region.
use criterion::{Criterion, criterion_group, criterion_main};
use lecop::{
    dataset::Dataset,
    validate::{validate, validate_with},
};
use std::{hint::black_box, path::Path};

#[path = "../tests/common/mod.rs"]
mod common;
use common::synth::{Synth, write_v2, write_v3};

fn bench(c: &mut Criterion) {
    let root = std::env::temp_dir().join("lecop-bench-validate");
    let _ = std::fs::remove_dir_all(&root);
    // 1,000 episodes x 600 frames (20 s at 30 fps), 14-dim action and state: 600k frames
    let s = Synth {
        episodes: 1000,
        frames: 600,
        dim: 14,
        fps: 30,
        per_file: 100,
    };
    let (v2, v3) = (root.join("v2"), root.join("v3"));
    write_v2(&v2, &s);
    write_v3(&v3, &s);

    let mut g = c.benchmark_group("validate");
    g.sample_size(10);
    let mut run = |name: &str, path: &Path| {
        let ds = Dataset::open(path).unwrap();
        assert!(validate(&ds).unwrap().is_empty(), "{name} must be healthy");
        g.bench_function(name, |b| b.iter(|| validate(black_box(&ds)).unwrap()));
    };
    run("synthetic_v2_1000ep_600k_frames", &v2);
    run("synthetic_v3_1000ep_600k_frames", &v3);
    for real in [
        "v3_aloha_sim_insertion_human",
        "svla_so101_pickplace",
        "c_careattrs_mug2v2",
    ] {
        let p = Path::new("datasets").join(real);
        if p.exists() {
            run(&format!("real_{real}"), &p);
        }
    }
    for (name, path) in [("v2", &v2), ("v3", &v3)] {
        let ds = Dataset::open(path).unwrap();
        g.bench_function(format!("synthetic_{name}_1000ep_600k_frames_jobs1"), |b| {
            b.iter(|| validate_with(black_box(&ds), 1).unwrap())
        });
    }
    g.finish();
    let _ = std::fs::remove_dir_all(&root);
}

criterion_group!(benches, bench);
criterion_main!(benches);
