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

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub timescale: u32,
    pub sample_count: u64,
    pub duration_ticks: u64,
    /// (width, height) from the first `stsd` sample entry, if the entry is long enough to hold them.
    pub size: Option<(u32, u32)>,
    /// `stts` runs: (sample count, ticks per sample). One run for a constant frame rate.
    runs: Vec<(u64, u64)>,
}

impl VideoInfo {
    pub fn duration_s(&self) -> f64 {
        self.duration_ticks as f64 / self.timescale as f64
    }

    pub fn fps(&self) -> f64 {
        self.sample_count as f64 / self.duration_s()
    }

    /// Samples whose decode time lies in `[from_s, to_s)`, with the span shifted back by half a frame so a
    /// frame stamped a hair after the boundary still belongs to its episode.
    // ponytail: decode time, not presentation time; B-frame reordering (ctts) moves a frame by at most a
    // few ticks but could in theory move one across a boundary. Read ctts if real data shows it.
    pub fn samples_in(&self, from_s: f64, to_s: f64) -> u64 {
        let ts = self.timescale as f64;
        let (mut start, mut n) = (0u64, 0u64);
        for &(count, delta) in &self.runs {
            let half = delta as f64 / 2.0;
            let (lo, hi) = (from_s * ts - half, to_s * ts - half);
            // sample i has time start + i*delta; count i in [0, count) with lo <= time < hi
            let first = (((lo - start as f64) / delta as f64).ceil()).max(0.0);
            let end = (((hi - start as f64) / delta as f64).ceil()).clamp(0.0, count as f64);
            if end > first {
                n += (end - first) as u64;
            }
            start += count * delta;
        }
        n
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
        if size > len - pos {
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

/// Width and height of a VisualSampleEntry: the first entry of `stsd` (8 bytes of table header, 8 of entry header,
/// then 24 bytes before the two 16-bit fields). None when the entry is too short, so unusual files are not rejected.
fn visual_size(stsd: &[u8]) -> Option<(u32, u32)> {
    let b = stsd.get(40..44)?;
    Some((
        u16::from_be_bytes([b[0], b[1]]) as u32,
        u16::from_be_bytes([b[2], b[3]]) as u32,
    ))
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
        let mut runs = Vec::new();
        for i in 0..be32(stts, 4)? as usize {
            let (count, delta) = (
                be32(stts, 8 + i * 8)? as u64,
                be32(stts, 12 + i * 8)? as u64,
            );
            stts_samples += count;
            ticks += count * delta;
            if delta > 0 {
                runs.push((count, delta));
            }
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
            size: child(stbl, b"stsd")?.and_then(visual_size),
            runs,
        });
    }
    bail!("no video track in moov")
}

/// How the samples of a video track are coded, from the `stsd` entry's configuration box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Codec {
    /// `avcC`: parameter sets and the byte width of the length prefix that starts each NAL unit in a sample.
    H264 {
        sps: Vec<Vec<u8>>,
        pps: Vec<Vec<u8>>,
        length_size: usize,
    },
    /// `av1C`: the sequence header OBUs.
    Av1 { config: Vec<u8> },
}

/// The coded samples of the first video track, in decode order.
pub struct Track {
    pub codec: Codec,
    pub samples: Vec<Vec<u8>>,
    /// Indices of the samples a decoder can start at, ascending (`stss`; only the first when the box is absent).
    pub sync: Vec<usize>,
}

/// Read every sample of the video track. Unlike [`read_video_info`] this touches all the frame data.
pub fn read_track(path: &Path) -> Result<Track> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let moov = read_moov(&mut f).with_context(|| format!("{}", path.display()))?;
    let Layout {
        codec,
        locations,
        sync,
    } = parse_track(&moov).with_context(|| format!("{}", path.display()))?;
    let len = f.metadata()?.len();
    let mut samples = Vec::with_capacity(locations.len());
    for (offset, size) in locations {
        if offset + size as u64 > len {
            bail!("sample at byte {offset} of {size} bytes runs past the end of the file");
        }
        f.seek(SeekFrom::Start(offset))?;
        let mut s = vec![0u8; size as usize];
        f.read_exact(&mut s)?;
        samples.push(s);
    }
    Ok(Track {
        codec,
        samples,
        sync,
    })
}

/// What the sample tables say about a track: how it is coded, where each sample is, which ones are sync samples.
struct Layout {
    codec: Codec,
    /// (offset, size) of every sample.
    locations: Vec<(u64, u32)>,
    sync: Vec<usize>,
}

fn parse_track(moov: &[u8]) -> Result<Layout> {
    for (kind, trak) in boxes(moov)? {
        if &kind != b"trak" {
            continue;
        }
        let mdia = need(trak, b"mdia")?;
        if need(mdia, b"hdlr")?.get(8..12) != Some(b"vide".as_slice()) {
            continue;
        }
        let stbl = need(need(mdia, b"minf")?, b"stbl")?;
        let codec = parse_codec(need(stbl, b"stsd")?)?;
        let locations = sample_locations(stbl)?;
        let sync = sync_samples(stbl)?;
        return Ok(Layout {
            codec,
            locations,
            sync,
        });
    }
    bail!("no video track in moov")
}

/// 0-based indices of the sync samples (the table is 1-based). A track without the table is only synced at its
/// start: nothing says decoding can begin elsewhere, so it is not split.
fn sync_samples(stbl: &[u8]) -> Result<Vec<usize>> {
    let Some(stss) = child(stbl, b"stss")? else {
        return Ok(vec![0]);
    };
    (0..be32(stss, 4)? as usize)
        .map(|i| Ok((be32(stss, 8 + i * 4)? as usize).saturating_sub(1)))
        .collect()
}

fn parse_codec(stsd: &[u8]) -> Result<Codec> {
    // 8 bytes of table header, 8 of entry header, 78 of VisualSampleEntry, then the codec's boxes.
    let kind = stsd.get(12..16).context("truncated stsd")?;
    let children = stsd
        .get(94..)
        .context("stsd entry has no codec configuration")?;
    match kind {
        b"avc1" | b"avc3" => {
            let c = need(children, b"avcC")?;
            let length_size = (c.get(4).context("truncated avcC")? & 3) as usize + 1;
            let mut at = 6;
            let sets = |n: usize, at: &mut usize| -> Result<Vec<Vec<u8>>> {
                (0..n)
                    .map(|_| {
                        let len = u16::from_be_bytes([
                            *c.get(*at).context("truncated avcC")?,
                            *c.get(*at + 1).context("truncated avcC")?,
                        ]) as usize;
                        let v = c.get(*at + 2..*at + 2 + len).context("truncated avcC")?;
                        *at += 2 + len;
                        Ok(v.to_vec())
                    })
                    .collect()
            };
            let sps = sets(
                (c.get(5).context("truncated avcC")? & 0x1f) as usize,
                &mut at,
            )?;
            let n_pps = *c.get(at).context("truncated avcC")? as usize;
            at += 1;
            let pps = sets(n_pps, &mut at)?;
            Ok(Codec::H264 {
                sps,
                pps,
                length_size,
            })
        }
        b"av01" => {
            let c = need(children, b"av1C")?;
            Ok(Codec::Av1 {
                config: c.get(4..).unwrap_or_default().to_vec(),
            })
        }
        other => bail!(
            "unsupported video codec `{}`",
            String::from_utf8_lossy(other)
        ),
    }
}

/// Resolve `stsc` (samples per chunk), `stco`/`co64` (chunk offsets) and `stsz` (sample sizes) to one
/// (offset, size) per sample.
fn sample_locations(stbl: &[u8]) -> Result<Vec<(u64, u32)>> {
    let stsz = need(stbl, b"stsz")?;
    let (uniform, count) = (be32(stsz, 4)?, be32(stsz, 8)? as usize);
    let size = |i: usize| -> Result<u32> {
        if uniform != 0 {
            Ok(uniform)
        } else {
            be32(stsz, 12 + i * 4)
        }
    };
    let chunk_offsets: Vec<u64> = match child(stbl, b"stco")? {
        Some(t) => (0..be32(t, 4)? as usize)
            .map(|i| be32(t, 8 + i * 4).map(u64::from))
            .collect::<Result<_>>()?,
        None => {
            let t = need(stbl, b"co64")?;
            (0..be32(t, 4)? as usize)
                .map(|i| {
                    let b = t.get(8 + i * 8..16 + i * 8).context("truncated co64")?;
                    Ok(u64::from_be_bytes(b.try_into()?))
                })
                .collect::<Result<_>>()?
        }
    };
    let stsc = need(stbl, b"stsc")?;
    let runs: Vec<(usize, usize)> = (0..be32(stsc, 4)? as usize)
        .map(|i| {
            Ok((
                be32(stsc, 8 + i * 12)? as usize,
                be32(stsc, 12 + i * 12)? as usize,
            ))
        })
        .collect::<Result<_>>()?;
    let mut out = Vec::with_capacity(count);
    for (chunk, &offset) in chunk_offsets.iter().enumerate() {
        let per = runs
            .iter()
            .rev()
            .find(|(first, _)| *first <= chunk + 1)
            .map_or(0, |r| r.1);
        let mut at = offset;
        for _ in 0..per {
            if out.len() == count {
                break;
            }
            let s = size(out.len())?;
            out.push((at, s));
            at += s as u64;
        }
    }
    if out.len() != count {
        bail!("sample tables place {} of {count} samples", out.len());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
        v.extend(kind);
        v.extend(payload);
        v
    }

    fn be(x: u32) -> Vec<u8> {
        x.to_be_bytes().to_vec()
    }

    #[test]
    fn reads_the_av1_configuration() {
        let mut entry = vec![0u8; 78];
        entry.extend(bx(b"av1C", &[0x81, 0, 0, 0, 10, 11]));
        let mut stsd = vec![0u8; 4];
        stsd.extend(be(1));
        stsd.extend(bx(b"av01", &entry));
        assert_eq!(
            parse_codec(&stsd).unwrap(),
            Codec::Av1 {
                config: vec![10, 11]
            }
        );
    }

    #[test]
    fn rejects_a_codec_it_cannot_decode() {
        let mut stsd = vec![0u8; 4];
        stsd.extend(be(1));
        stsd.extend(bx(b"hvc1", &[0u8; 78]));
        assert!(parse_codec(&stsd).unwrap_err().to_string().contains("hvc1"));
    }

    #[test]
    fn places_samples_from_chunks_with_64_bit_offsets() {
        let mut stsz = vec![0u8; 4];
        stsz.extend([be(0), be(3), be(10), be(20), be(30)].concat());
        // Two chunks: the first holds two samples, the second one.
        let mut stsc = vec![0u8; 4];
        stsc.extend([be(2), be(1), be(2), be(1), be(2), be(1), be(1), be(1)].concat());
        let mut co64 = vec![0u8; 4];
        co64.extend(be(2));
        co64.extend(1000u64.to_be_bytes());
        co64.extend(5000u64.to_be_bytes());
        let stbl = [bx(b"stsz", &stsz), bx(b"stsc", &stsc), bx(b"co64", &co64)].concat();
        assert_eq!(
            sample_locations(&stbl).unwrap(),
            [(1000, 10), (1010, 20), (5000, 30)]
        );
    }

    #[test]
    fn rejects_tables_that_place_fewer_samples_than_counted() {
        let mut stsz = vec![0u8; 4];
        stsz.extend([be(7), be(3)].concat());
        let mut stsc = vec![0u8; 4];
        stsc.extend([be(1), be(1), be(1), be(1)].concat());
        let mut stco = vec![0u8; 4];
        stco.extend([be(1), be(100)].concat());
        let stbl = [bx(b"stsz", &stsz), bx(b"stsc", &stsc), bx(b"stco", &stco)].concat();
        assert!(sample_locations(&stbl).is_err());
    }

    #[test]
    fn sync_samples_are_zero_based_and_default_to_the_start() {
        let mut stss = vec![0u8; 4];
        stss.extend(be(2));
        stss.extend(be(1));
        stss.extend(be(31));
        assert_eq!(
            sync_samples(&bx(b"stss", &stss)).unwrap(),
            [0, 30],
            "the table counts samples from 1"
        );
        assert_eq!(sync_samples(&bx(b"stsz", &[])).unwrap(), [0]);
    }
}
