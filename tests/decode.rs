//! `--decode` on H.264 videos made with openh264's own encoder, muxed into a minimal mp4, and on one real AV1
//! video when `datasets/` has it.
#![cfg(feature = "decode")]
mod common;
use common::h264::{video, video_with_idr_every};
use lecop::decode::{Decoded, decode_checked, decode_segmented, video_findings};
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

/// A varied picture sequence: changing levels, a still stretch and a black one, so the differences between
/// neighbouring pictures, including across a segment boundary, take many values.
fn varied() -> Vec<(u8, u8)> {
    (0..120)
        .map(|i| match i {
            40..=59 => (100, 0),
            80..=99 => (0, 0),
            _ => (60 + (i % 7) as u8 * 20, 2 + (i % 3) as u8),
        })
        .collect()
}

#[test]
fn decoding_in_segments_gives_the_same_frames_as_decoding_in_one() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.mp4");
    video_with_idr_every(&p, &varied(), None, 10);
    let sync = lecop::mp4::read_track(&p).unwrap().sync;
    assert!(
        sync.len() > 5,
        "the video should have many sync samples: {sync:?}"
    );
    let whole = decode_segmented(&p, usize::MAX).unwrap();
    assert_eq!(whole.frames.len(), 120, "{whole:?}");
    for target in [1, 10, 25, 64] {
        let split = decode_segmented(&p, target).unwrap();
        assert_eq!(split.frames, whole.frames, "target {target}");
        assert_eq!(
            (split.samples, split.failures),
            (whole.samples, whole.failures)
        );
    }
}

#[test]
fn a_failure_in_a_later_segment_is_reported_at_its_position_in_the_video() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.mp4");
    video_with_idr_every(&p, &[(100, 3); 60], Some(45), 10);
    let split = decode_segmented(&p, 10).unwrap();
    assert!(split.failures > 0, "{split:?}");
    assert!(split.first_failure.is_some_and(|f| f >= 45), "{split:?}");
}

/// 60 frames of 128x128 test pattern (SVT-AV1, a keyframe every 15), made with ffmpeg; the one AV1 stream the tests
/// can decode without `datasets/`.
const AV1: &str = "tests/fixtures/av1_60_frames.mp4";

#[test]
fn an_av1_video_decodes_every_frame() {
    let d = decode_checked(Path::new(AV1)).unwrap();
    assert_eq!(
        (d.failures, d.samples, d.frames.len()),
        (0, 60, 60),
        "{d:?}"
    );
    assert!(video_findings(&d).is_empty(), "{:?}", video_findings(&d));
}

#[test]
fn av1_decoding_in_segments_gives_the_same_frames_as_decoding_in_one() {
    let p = Path::new(AV1);
    let sync = lecop::mp4::read_track(p).unwrap().sync;
    assert_eq!(sync, [0, 15, 30, 45]);
    let whole = decode_segmented(p, usize::MAX).unwrap();
    for target in [1, 15, 20] {
        let split = decode_segmented(p, target).unwrap();
        assert_eq!(split.frames, whole.frames, "target {target}");
        assert_eq!(split.failures, 0);
    }
}

// A real-world file; slow in a debug build: cargo test --release --features decode -- --ignored
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
