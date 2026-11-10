//! `--decode` on H.264 videos made with openh264's own encoder, muxed into a minimal mp4, and on one real AV1
//! video when `datasets/` has it.
#![cfg(feature = "decode")]
mod common;
use common::h264::video;
use lecop::decode::{Decoded, decode_checked, video_findings};
use std::path::Path;

fn findings(frames: &[(u8, u8)], corrupt_from: Option<usize>) -> (Decoded, Vec<&'static str>) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.mp4");
    video(&p, frames, corrupt_from);
    let d = decode_checked(&p).unwrap();
    let codes = video_findings(&d).into_iter().map(|f| f.0).collect();
    (d, codes)
}

#[test]
fn a_moving_picture_has_no_findings() {
    let frames = vec![(100, 3); 40];
    let (d, codes) = findings(&frames, None);
    assert_eq!(d.frames.len(), 40);
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn a_repeated_picture_is_frozen() {
    let mut frames = vec![(100, 3); 10];
    frames.extend(vec![(100, 0); 40]);
    let (_, codes) = findings(&frames, None);
    assert_eq!(codes, ["video_frozen"]);
}

#[test]
fn a_black_picture_is_black() {
    let mut frames = vec![(100, 3); 10];
    frames.extend(vec![(0, 0); 40]);
    let (_, codes) = findings(&frames, None);
    assert!(codes.contains(&"video_black"), "{codes:?}");
}

#[test]
fn garbage_samples_are_undecodable() {
    let (d, codes) = findings(&[(100, 3); 40], Some(10));
    assert!(d.failures > 0, "{d:?}");
    assert!(codes.contains(&"video_decode"), "{codes:?}");
}

// rav1d without assembly takes minutes per video in a debug build: cargo test --release --features decode -- --ignored
#[test]
#[ignore = "slow in debug builds"]
fn a_real_av1_video_decodes_every_frame() {
    let p = Path::new(
        "datasets/c_careattrs_mug2v2/videos/chunk-000/observation.images.front/episode_000000.mp4",
    );
    if !p.exists() {
        eprintln!("skipping: datasets/c_careattrs_mug2v2 not downloaded");
        return;
    }
    let d = decode_checked(p).unwrap();
    assert_eq!(d.failures, 0, "{d:?}");
    assert_eq!(d.frames.len(), d.samples);
}
