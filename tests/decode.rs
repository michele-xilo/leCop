//! `--decode` on H.264 videos made with openh264's own encoder, muxed into a minimal mp4, and on one real AV1
//! video when `datasets/` has it.
#![cfg(feature = "decode")]
use lecop::decode::{Decoded, decode_checked, video_findings};
use openh264::{
    encoder::{Encoder, EncoderConfig},
    formats::YUVBuffer,
};
use std::{fs, path::Path};

const W: usize = 64;
const H: usize = 64;

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(kind);
    v.extend(payload);
    v
}

fn be(x: u32) -> [u8; 4] {
    x.to_be_bytes()
}

/// An mp4 holding `samples` (each already length-prefixed) as one H.264 video track.
fn mp4(samples: &[Vec<u8>], sps: &[u8], pps: &[u8]) -> Vec<u8> {
    let ftyp = bx(b"ftyp", b"isom\0\0\0\0isom");
    let mdat = bx(b"mdat", &samples.concat());
    let first = (ftyp.len() + 8) as u32;

    let mut avcc = vec![1, sps[1], sps[2], sps[3], 0xff, 0xe1];
    avcc.extend((sps.len() as u16).to_be_bytes());
    avcc.extend(sps);
    avcc.push(1);
    avcc.extend((pps.len() as u16).to_be_bytes());
    avcc.extend(pps);
    let mut entry = vec![0u8; 6];
    entry.extend([0, 1]);
    entry.extend([0u8; 16]);
    entry.extend((W as u16).to_be_bytes());
    entry.extend((H as u16).to_be_bytes());
    entry.extend(vec![0u8; 50]);
    entry.extend(bx(b"avcC", &avcc));
    let mut stsd = vec![0u8; 4];
    stsd.extend(be(1));
    stsd.extend(bx(b"avc1", &entry));

    let n = samples.len() as u32;
    let mut stts = vec![0u8; 4];
    stts.extend(be(1));
    stts.extend(be(n));
    stts.extend(be(1000));
    let mut stsc = vec![0u8; 4];
    stsc.extend(be(1));
    stsc.extend([be(1), be(n), be(1)].concat());
    let mut stsz = vec![0u8; 4];
    stsz.extend(be(0));
    stsz.extend(be(n));
    for s in samples {
        stsz.extend(be(s.len() as u32));
    }
    let mut stco = vec![0u8; 4];
    stco.extend(be(1));
    stco.extend(be(first));
    let stbl = bx(
        b"stbl",
        &[
            bx(b"stsd", &stsd),
            bx(b"stts", &stts),
            bx(b"stsc", &stsc),
            bx(b"stsz", &stsz),
            bx(b"stco", &stco),
        ]
        .concat(),
    );
    let mut mdhd = vec![0u8; 12];
    mdhd.extend(be(30_000));
    mdhd.extend(be(n * 1000));
    mdhd.extend([0u8; 4]);
    let mut hdlr = vec![0u8; 8];
    hdlr.extend(b"vide");
    hdlr.extend([0u8; 13]);
    let mdia = bx(
        b"mdia",
        &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), bx(b"minf", &stbl)].concat(),
    );
    let moov = bx(b"moov", &bx(b"trak", &mdia));
    [ftyp, mdat, moov].concat()
}

/// Split an Annex B stream into its NAL units.
fn nals(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = vec![];
    let mut i = 0;
    while i + 3 <= stream.len() {
        if stream[i..i + 3] == [0, 0, 1] {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    (0..starts.len())
        .map(|k| {
            let end = starts.get(k + 1).map_or(stream.len(), |s| s - 3);
            let nal = &stream[starts[k]..end];
            // A four-byte start code leaves a zero at the end of the previous unit.
            nal.strip_suffix(&[0]).unwrap_or(nal)
        })
        .collect()
}

/// Encode `luma` frames (flat 64x64 pictures, one level each, plus per-frame noise) and write them as an mp4.
fn video(path: &Path, frames: &[(u8, u8)], corrupt_from: Option<usize>) {
    let mut enc = Encoder::with_api_config(
        openh264::OpenH264API::from_source(),
        EncoderConfig::new().skip_frames(false),
    )
    .unwrap();
    let (mut sps, mut pps) = (vec![], vec![]);
    let mut samples = vec![];
    for (i, &(level, noise)) in frames.iter().enumerate() {
        let mut yuv = vec![128u8; W * H * 3 / 2];
        for (p, v) in yuv[..W * H].iter_mut().enumerate() {
            // A different pattern per frame with `noise` > 0; identical pictures when it is 0.
            *v = level.wrapping_add(((p * 7 + i * 13) % 31) as u8 * noise);
        }
        let buf = YUVBuffer::from_vec(yuv, W, H);
        let stream = enc.encode(&buf).unwrap().to_vec();
        let mut sample = vec![];
        for nal in nals(&stream) {
            match nal[0] & 0x1f {
                7 => sps = nal.to_vec(),
                8 => pps = nal.to_vec(),
                _ => {
                    sample.extend(be(nal.len() as u32));
                    sample.extend(nal);
                }
            }
        }
        if corrupt_from.is_some_and(|c| i >= c) {
            sample = (0..sample.len()).map(|b| (b * 31 % 251) as u8).collect();
        }
        samples.push(sample);
    }
    fs::write(path, mp4(&samples, &sps, &pps)).unwrap();
}

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
