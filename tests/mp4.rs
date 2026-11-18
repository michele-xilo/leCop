//! The mp4 reader must get frame count and duration from container metadata alone.
mod common;
use common::Mp4;
use lecop::mp4::read_video_info;
use std::fs;

fn read(bytes: Vec<u8>) -> anyhow::Result<lecop::mp4::VideoInfo> {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("v.mp4");
    fs::write(&p, bytes).unwrap();
    read_video_info(&p)
}

#[test]
fn reads_frame_count_and_duration_when_moov_is_last() {
    let i = read(Mp4::constant_fps(161, 10).bytes()).unwrap();
    assert_eq!(i.sample_count, 161);
    assert!((i.duration_s() - 16.1).abs() < 1e-9, "{}", i.duration_s());
    assert!((i.fps() - 10.0).abs() < 1e-9);
}

#[test]
fn reads_the_same_when_moov_is_first() {
    let mut m = Mp4::constant_fps(30, 30);
    m.moov_first = true;
    let i = read(m.bytes()).unwrap();
    assert_eq!(i.sample_count, 30);
    assert!((i.duration_s() - 1.0).abs() < 1e-9);
}

#[test]
fn skips_an_audio_track_that_comes_first() {
    let mut m = Mp4::constant_fps(20, 10);
    m.audio_track_first = true;
    let i = read(m.bytes()).unwrap();
    assert_eq!(i.sample_count, 20);
}

#[test]
fn sums_multiple_stts_runs_for_variable_frame_rate() {
    let mut m = Mp4::constant_fps(0, 10);
    m.timescale = 1000;
    m.stts = vec![(10, 100), (10, 200)]; // 1.0 s then 2.0 s
    let i = read(m.bytes()).unwrap();
    assert_eq!(i.sample_count, 20);
    assert!((i.duration_s() - 3.0).abs() < 1e-9);
}

#[test]
fn handles_64_bit_box_sizes() {
    let mut m = Mp4::constant_fps(5, 10);
    m.large_mdat = true;
    assert_eq!(read(m.bytes()).unwrap().sample_count, 5);
}

#[test]
fn errors_when_there_is_no_video_track() {
    let mut m = Mp4::constant_fps(5, 10);
    m.video_track = false;
    let e = format!("{:#}", read(m.bytes()).unwrap_err());
    assert!(e.contains("video track"), "{e}");
}

#[test]
fn errors_on_truncated_file() {
    let b = Mp4::constant_fps(50, 10).bytes();
    let e = format!("{:#}", read(b[..b.len() - 30].to_vec()).unwrap_err());
    assert!(e.contains("truncated") || e.contains("moov"), "{e}");
}

#[test]
fn errors_on_empty_and_garbage_files() {
    assert!(read(vec![]).is_err());
    assert!(read(b"this is not an mp4 file at all".to_vec()).is_err());
}

#[test]
fn errors_instead_of_overflowing_on_a_huge_64_bit_box_size() {
    let mut b = Mp4::constant_fps(1, 10).bytes()[..20].to_vec(); // ftyp only
    b.extend([0, 0, 0, 1]); // size == 1 -> 64-bit size follows
    b.extend(b"free");
    b.extend(u64::MAX.to_be_bytes());
    assert!(read(b).is_err());
}

#[test]
fn counts_samples_whose_time_falls_inside_a_span() {
    let i = read(Mp4::constant_fps(30, 10).bytes()).unwrap();
    assert_eq!(i.samples_in(0.0, 1.0), 10);
    assert_eq!(i.samples_in(1.0, 2.0), 10);
    assert_eq!(i.samples_in(2.0, 5.0), 10); // clipped at the end of the video
    assert_eq!(i.samples_in(5.0, 6.0), 0);
}

#[test]
fn counts_samples_across_runs_with_different_frame_durations() {
    // 10 frames at 10 fps, then 10 frames at 5 fps (dropped frames): the second run covers 1 s to 3 s.
    let mut m = Mp4::constant_fps(0, 10);
    m.stts = vec![(10, 1024), (10, 2048)];
    let i = read(m.bytes()).unwrap();
    assert_eq!(i.samples_in(0.0, 1.0), 10);
    assert_eq!(i.samples_in(1.0, 2.0), 5);
    assert_eq!(i.samples_in(1.0, 3.0), 10);
}

/// 1 + 4 * groups frames at 30 fps (512 ticks at timescale 15360) with B-frames, tables as ffmpeg writes them
/// (from a real H.264 LeRobot video): irregular decode times, and composition offsets that put every frame's
/// presentation time on a multiple of 512. `shift` is added to every offset and cancelled by an edit list.
fn b_frames(groups: u32, shift: u32) -> Mp4 {
    let mut m = Mp4::constant_fps(0, 30);
    m.timescale = 15360;
    m.stts = vec![(1, 510)];
    m.ctts = vec![(1, shift)];
    for g in 0..groups {
        m.stts.extend([
            (2, 1),
            (1, 1024),
            (1, if g + 1 == groups { 512 } else { 1022 }),
        ]);
        m.ctts
            .extend([(1, 1538 + shift), (1, 513 + shift), (2, shift)]);
    }
    m.edit_media_time = (shift > 0).then_some(shift);
    m
}

#[test]
fn counts_b_frame_videos_by_presentation_time() {
    for shift in [0, 1024] {
        let n = 1 + 4 * 50;
        let i = read(b_frames(50, shift).bytes()).unwrap();
        assert_eq!(i.sample_count, n as u64);
        for m in 1..=n {
            assert_eq!(
                i.samples_in(0.0, m as f64 / 30.0),
                m as u64,
                "shift {shift}, span of {m} frames"
            );
        }
        assert!(
            (i.duration_s() - n as f64 / 30.0).abs() < 1e-9,
            "shift {shift}: {}",
            i.duration_s()
        );
    }
}

#[test]
fn reads_the_frame_size_from_the_sample_description() {
    let mut m = Mp4::constant_fps(10, 10);
    m.size = Some((640, 480));
    assert_eq!(read(m.bytes()).unwrap().size, Some((640, 480)));
}

#[test]
fn has_no_frame_size_without_a_sample_description() {
    assert_eq!(read(Mp4::constant_fps(10, 10).bytes()).unwrap().size, None);
}
