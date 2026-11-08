//! Read frame count and duration from an mp4's container metadata (`moov`) without touching frame data.
//!
//! Hand-written instead of using the `mp4` crate: LeRobot videos are AV1, the `moov` box sits at the end
//! of the file after a multi-MB `mdat`, and we need only the sample tables. We seek past `mdat` and read
//! just `moov`, so cost is independent of video size.
use anyhow::{Context, Result, bail};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

/// Real `moov` boxes are well under a few MB even for hour-long videos; anything bigger is corrupt.
const MAX_MOOV_BYTES: u64 = 512 << 20;

#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub timescale: u32,
    pub sample_count: u64,
    pub duration_ticks: u64,
}

impl VideoInfo {
    pub fn duration_s(&self) -> f64 {
        self.duration_ticks as f64 / self.timescale as f64
    }

    pub fn fps(&self) -> f64 {
        self.sample_count as f64 / self.duration_s()
    }
}

pub fn read_video_info(path: &Path) -> Result<VideoInfo> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let moov = read_moov(&mut f).with_context(|| format!("{}", path.display()))?;
    parse_moov(&moov).with_context(|| format!("{}", path.display()))
}

/// Scan top-level boxes by header only, seeking over payloads, and return the `moov` payload.
fn read_moov(f: &mut File) -> Result<Vec<u8>> {
    let len = f.metadata()?.len();
    let mut pos = 0u64;
    while pos < len {
        f.seek(SeekFrom::Start(pos))?;
        let mut h = [0u8; 8];
        if f.read_exact(&mut h).is_err() {
            bail!("truncated box header at byte {pos}");
        }
        let mut size = u32::from_be_bytes([h[0], h[1], h[2], h[3]]) as u64;
        let kind = [h[4], h[5], h[6], h[7]];
        let mut header = 8;
        if size == 1 {
            let mut big = [0u8; 8];
            if f.read_exact(&mut big).is_err() {
                bail!("truncated 64-bit box header at byte {pos}");
            }
            size = u64::from_be_bytes(big);
            header = 16;
        } else if size == 0 {
            size = len - pos; // box runs to end of file
        }
        if size < header {
            bail!("invalid box size {size} at byte {pos}");
        }
        if pos + size > len {
            bail!(
                "truncated {} box at byte {pos}: needs {size} bytes, file has {}",
                String::from_utf8_lossy(&kind),
                len - pos
            );
        }
        if &kind == b"moov" {
            let n = size - header;
            if n > MAX_MOOV_BYTES {
                bail!("moov box of {n} bytes is implausibly large");
            }
            let mut buf = vec![0u8; n as usize];
            f.seek(SeekFrom::Start(pos + header))?;
            f.read_exact(&mut buf)?;
            return Ok(buf);
        }
        pos += size;
    }
    bail!("no moov box (file truncated or not an mp4)")
}

type BoxRef<'a> = ([u8; 4], &'a [u8]);

fn boxes(mut buf: &[u8]) -> Result<Vec<BoxRef<'_>>> {
    let mut out = Vec::new();
    while !buf.is_empty() {
        if buf.len() < 8 {
            bail!("truncated box header inside moov");
        }
        let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
        let kind = [buf[4], buf[5], buf[6], buf[7]];
        // ponytail: 64-bit sizes and size 0 are only handled at top level; inside moov they do not occur.
        if size < 8 || size > buf.len() {
            bail!(
                "invalid {} box size {size} inside moov",
                String::from_utf8_lossy(&kind)
            );
        }
        out.push((kind, &buf[8..size]));
        buf = &buf[size..];
    }
    Ok(out)
}

fn child<'a>(parent: &'a [u8], kind: &[u8; 4]) -> Result<Option<&'a [u8]>> {
    Ok(boxes(parent)?
        .into_iter()
        .find(|(k, _)| k == kind)
        .map(|(_, p)| p))
}

fn need<'a>(parent: &'a [u8], kind: &[u8; 4]) -> Result<&'a [u8]> {
    child(parent, kind)?.with_context(|| format!("missing {} box", String::from_utf8_lossy(kind)))
}

fn be32(b: &[u8], off: usize) -> Result<u32> {
    let s = b.get(off..off + 4).context("truncated box payload")?;
    Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn parse_moov(moov: &[u8]) -> Result<VideoInfo> {
    for (kind, trak) in boxes(moov)? {
        if &kind != b"trak" {
            continue;
        }
        let mdia = need(trak, b"mdia")?;
        let hdlr = need(mdia, b"hdlr")?;
        if hdlr.get(8..12) != Some(b"vide".as_slice()) {
            continue;
        }
        let mdhd = need(mdia, b"mdhd")?;
        let timescale = match mdhd.first() {
            Some(1) => be32(mdhd, 20)?, // version 1: 64-bit creation and modification times
            _ => be32(mdhd, 12)?,
        };
        if timescale == 0 {
            bail!("video track has timescale 0");
        }
        let stbl = need(need(mdia, b"minf")?, b"stbl")?;

        let stts = need(stbl, b"stts")?;
        let (mut stts_samples, mut ticks) = (0u64, 0u64);
        for i in 0..be32(stts, 4)? as usize {
            let (count, delta) = (
                be32(stts, 8 + i * 8)? as u64,
                be32(stts, 12 + i * 8)? as u64,
            );
            stts_samples += count;
            ticks += count * delta;
        }
        let sample_count = be32(need(stbl, b"stsz")?, 8)? as u64;
        if sample_count != stts_samples {
            bail!(
                "inconsistent sample tables: stsz has {sample_count} samples, stts has {stts_samples}"
            );
        }
        return Ok(VideoInfo {
            timescale,
            sample_count,
            duration_ticks: ticks,
        });
    }
    bail!("no video track in moov")
}
