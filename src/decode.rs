//! Opt-in frame decoding (`--decode`, cargo feature `decode`): undecodable, frozen and black camera streams.
//! The default build never decodes; everything here except the decoder backends is plain arithmetic on
//! per-frame statistics so it can be tested without a video.

/// A frame of the stream, reduced to what the checks need.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameStat {
    /// Mean luma over the sampled pixels, 0..=255.
    pub luma: f32,
    /// Mean absolute luma difference to the previous frame; 0 for the first.
    pub diff: f32,
}

/// What decoding one video gave.
#[derive(Debug, Default)]
pub struct Decoded {
    pub frames: Vec<FrameStat>,
    /// Samples (coded frames) the container lists.
    pub samples: usize,
    /// Samples the decoder rejected, and the first one's position.
    pub failures: usize,
    pub first_failure: Option<usize>,
}

/// A run this long (about a second of 30 fps video) is a stalled or covered camera, not a pause in the motion.
pub const RUN: usize = 30;
/// Mean luma under which a frame counts as black.
const BLACK_LUMA: f32 = 8.0;
/// Every `STRIDE`th row and column is looked at: the checks need the picture's level and whether it moved.
const STRIDE: usize = 4;

/// Luma of every `STRIDE`th row and column of a plane with `stride` bytes per row.
pub fn sample_luma(plane: &[u8], stride: usize, w: usize, h: usize) -> Vec<u8> {
    (0..h)
        .step_by(STRIDE)
        .flat_map(|r| {
            plane
                .get(r * stride..r * stride + w)
                .unwrap_or_default()
                .iter()
                .step_by(STRIDE)
                .copied()
        })
        .collect()
}

/// Statistics of a sampled frame against the previous sampled frame, when it has the same size.
pub fn frame_stat(cur: &[u8], prev: Option<&[u8]>) -> FrameStat {
    let n = cur.len().max(1) as f32;
    let luma = cur.iter().map(|&v| v as f32).sum::<f32>() / n;
    let diff = match prev {
        Some(p) if p.len() == cur.len() => {
            cur.iter()
                .zip(p)
                .map(|(&a, &b)| a.abs_diff(b) as f32)
                .sum::<f32>()
                / n
        }
        _ => 0.0,
    };
    FrameStat { luma, diff }
}

/// (start, length) of every run of at least [`RUN`] consecutive frames satisfying `pred`.
fn runs(frames: &[FrameStat], pred: impl Fn(&FrameStat) -> bool) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    // A sentinel frame that satisfies no predicate closes a run that reaches the end.
    let end = FrameStat {
        luma: 255.0,
        diff: 1.0,
    };
    for (i, f) in frames.iter().chain(std::iter::once(&end)).enumerate() {
        match (pred(f), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                if i - s >= RUN {
                    out.push((s, i - s));
                }
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// Issue codes and messages for one decoded video.
pub fn video_findings(d: &Decoded) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    let missing = d.samples.saturating_sub(d.frames.len());
    if d.failures > 0 || missing > 0 {
        let first = d
            .first_failure
            .map_or(String::new(), |f| format!(", the first at frame {f}"));
        out.push((
            "video_decode",
            format!(
                "{} of {} frames could not be decoded{first}",
                d.failures.max(missing),
                d.samples
            ),
        ));
    }
    // diff is exactly 0 only for a repeated picture: a live camera's sensor noise survives compression.
    for (code, what, found) in [
        (
            "video_frozen",
            "identical to the one before",
            runs(&d.frames, |f| f.diff == 0.0),
        ),
        (
            "video_black",
            "black",
            runs(&d.frames, |f| f.luma < BLACK_LUMA),
        ),
    ] {
        if let Some(&(start, len)) = found.iter().max_by_key(|r| r.1) {
            let more = if found.len() > 1 {
                format!(" (longest of {} runs)", found.len())
            } else {
                String::new()
            };
            out.push((
                code,
                format!("{len} frames in a row are {what}, from frame {start}{more}"),
            ));
        }
    }
    out
}

/// Target length of a segment decoded on its own thread, in samples. Long enough that a task costs far more than
/// scheduling it (a frame takes about 2 ms), short enough that one file keeps every core busy.
#[cfg(feature = "decode")]
const SEGMENT: usize = 250;

/// Split `0..total` at sync samples into ranges of at least `target` samples, except the last. A decoder can start
/// at any sync sample, so the ranges decode independently of each other.
#[cfg(any(feature = "decode", test))]
fn segments(sync: &[usize], total: usize, target: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for &s in sync {
        if s > start && s - start >= target && s < total {
            out.push(start..s);
            start = s;
        }
    }
    out.push(start..total);
    out
}

/// Decode the video at `path`, a task per segment of at least `target` samples; the result does not depend on
/// `target`. Public so that tests and benchmarks can choose the segment length.
/// True when rav1d's assembly is compiled in: always on aarch64 and on x86_64 Linux, Windows and macOS (see
/// Cargo.toml), elsewhere only with `asm`.
pub const AV1_ASM: bool = cfg!(any(
    target_arch = "aarch64",
    all(
        target_arch = "x86_64",
        any(
            target_os = "linux",
            target_os = "windows",
            target_os = "macos"
        )
    ),
    feature = "asm"
));

/// The stderr note for a `--decode` run on a build whose AV1 decoder has no assembly (about 3x slower).
pub fn asm_note(decoding: bool, asm: bool) -> Option<&'static str> {
    (decoding && !asm).then_some(
        "note: this build decodes AV1 without assembly (about 3x slower); the release binaries, PyPI wheels and \
         cargo binstall have it, or rebuild with --features asm and nasm installed",
    )
}

#[cfg(feature = "decode")]
pub fn decode_segmented(path: &std::path::Path, target: usize) -> anyhow::Result<Decoded> {
    backend::decode_with(path, target)
}

/// Decode the video at `path`; an error says why it was not decoded.
#[cfg(feature = "decode")]
pub fn decode_checked(path: &std::path::Path) -> anyhow::Result<Decoded> {
    backend::decode_video(path)
}

#[cfg(not(feature = "decode"))]
pub fn decode_checked(_: &std::path::Path) -> anyhow::Result<Decoded> {
    anyhow::bail!("this build cannot decode; rebuild with `--features decode`")
}

#[cfg(feature = "decode")]
mod backend {
    use super::{Decoded, SEGMENT, frame_stat, sample_luma, segments};
    use crate::mp4::{Codec, Track, read_track};
    use anyhow::{Context, Result};
    use rayon::prelude::*;
    use std::{ops::Range, path::Path};

    /// Called with each decoded luma plane: (plane, bytes per row, width, height).
    type Push<'a> = dyn FnMut(&mut Decoded, &[u8], usize, usize, usize) + 'a;

    /// One segment's result, with the sampled first and last pictures so that segments can be joined.
    struct Part {
        d: Decoded,
        first: Option<Vec<u8>>,
        last: Option<Vec<u8>>,
    }

    /// Decode every frame of the video at `path`, a segment per task.
    pub fn decode_video(path: &Path) -> Result<Decoded> {
        decode_with(path, SEGMENT)
    }

    pub(super) fn decode_with(path: &Path, target: usize) -> Result<Decoded> {
        let track = read_track(path)?;
        let parts = segments(&track.sync, track.samples.len(), target)
            .into_par_iter()
            .map(|r| decode_range(&track, r))
            .collect::<Result<Vec<_>>>()?;
        let mut out = Decoded::default();
        let mut prev: Option<Vec<u8>> = None;
        for mut p in parts {
            // A segment starts with no previous picture: the difference to the last one before it is added here.
            if let (Some(first), Some(prev)) = (&p.first, &prev)
                && let Some(f) = p.d.frames.first_mut()
            {
                f.diff = frame_stat(first, Some(prev)).diff;
            }
            if p.last.is_some() {
                prev = p.last;
            }
            out.samples += p.d.samples;
            out.failures += p.d.failures;
            out.first_failure = out.first_failure.or(p.d.first_failure);
            out.frames.append(&mut p.d.frames);
        }
        Ok(out)
    }

    fn decode_range(track: &Track, range: Range<usize>) -> Result<Part> {
        let base = range.start;
        let samples = &track.samples[range];
        let mut d = Decoded {
            samples: samples.len(),
            ..Decoded::default()
        };
        let (mut first, mut prev): (Option<Vec<u8>>, Option<Vec<u8>>) = (None, None);
        let mut push = |d: &mut Decoded, plane: &[u8], stride: usize, w: usize, h: usize| {
            let cur = sample_luma(plane, stride, w, h);
            d.frames.push(frame_stat(&cur, prev.as_deref()));
            if first.is_none() {
                first = Some(cur.clone());
            }
            prev = Some(cur);
        };
        match &track.codec {
            Codec::H264 {
                sps,
                pps,
                length_size,
            } => h264(samples, base, sps, pps, *length_size, &mut d, &mut push)?,
            Codec::Av1 { config } => av1(samples, base, config, &mut d, &mut push)?,
        }
        Ok(Part {
            d,
            first,
            last: prev,
        })
    }

    fn h264(
        samples: &[Vec<u8>],
        base: usize,
        sps: &[Vec<u8>],
        pps: &[Vec<u8>],
        length_size: usize,
        d: &mut Decoded,
        push: &mut Push,
    ) -> Result<()> {
        use openh264::{
            OpenH264API,
            decoder::{Decoder, DecoderConfig, Flush},
            formats::YUVSource,
        };
        const START: [u8; 4] = [0, 0, 0, 1];
        // The default flushes after every packet, which fails on frames coded as several slices.
        let config = DecoderConfig::new().flush_after_decode(Flush::NoFlush);
        let mut dec = Decoder::with_api_config(OpenH264API::from_source(), config)
            .context("creating the H.264 decoder")?;
        let mut headers = Vec::new();
        for set in sps.iter().chain(pps) {
            headers.extend_from_slice(&START);
            headers.extend_from_slice(set);
        }
        for (i, sample) in samples.iter().enumerate() {
            // The container prefixes each NAL unit with its length; the decoder wants start codes.
            let mut annex_b = if i == 0 { headers.clone() } else { Vec::new() };
            let (mut at, mut ok) = (0, true);
            while at + length_size <= sample.len() {
                let len = sample[at..at + length_size]
                    .iter()
                    .fold(0usize, |a, &b| a << 8 | b as usize);
                at += length_size;
                match sample.get(at..at + len) {
                    Some(nal) => {
                        annex_b.extend_from_slice(&START);
                        annex_b.extend_from_slice(nal);
                    }
                    None => ok = false,
                }
                at += len;
            }
            match ok.then(|| dec.decode(&annex_b)) {
                Some(Ok(Some(yuv))) => {
                    let (w, h) = yuv.dimensions();
                    push(d, yuv.y(), yuv.strides().0, w, h);
                }
                Some(Ok(None)) => {}
                _ => {
                    d.failures += 1;
                    d.first_failure.get_or_insert(base + i);
                }
            }
        }
        if let Ok(rest) = dec.flush_remaining() {
            for yuv in rest {
                let (w, h) = yuv.dimensions();
                push(d, yuv.y(), yuv.strides().0, w, h);
            }
        }
        Ok(())
    }

    fn av1(
        samples: &[Vec<u8>],
        base: usize,
        config: &[u8],
        d: &mut Decoded,
        push: &mut Push,
    ) -> Result<()> {
        let mut dec = Av1::open()?;
        dec.send(config, &mut |_, _, _, _| {});
        for (i, sample) in samples.iter().enumerate() {
            if !dec.send(sample, &mut |p, s, w, h| push(d, p, s, w, h)) {
                d.failures += 1;
                d.first_failure.get_or_insert(base + i);
            }
        }
        dec.drain(&mut |p, s, w, h| push(d, p, s, w, h));
        Ok(())
    }

    /// rav1d exposes only its C API, so this is the one place in lecop with unsafe code.
    struct Av1 {
        ctx: Option<rav1d::include::dav1d::dav1d::Dav1dContext>,
    }

    type Plane<'a> = dyn FnMut(&[u8], usize, usize, usize) + 'a;

    impl Av1 {
        fn open() -> Result<Self> {
            use rav1d::{
                include::dav1d::dav1d::Dav1dSettings,
                {dav1d_default_settings, dav1d_open},
            };
            use std::{mem::MaybeUninit, ptr::NonNull};
            let mut settings = MaybeUninit::<Dav1dSettings>::uninit();
            let mut ctx = None;
            // SAFETY: `dav1d_default_settings` fully initializes `settings`, which is valid to write to; `ctx`
            // is valid to write to; both outlive the calls.
            let r = unsafe {
                dav1d_default_settings(NonNull::from(&mut settings).cast());
                let s = &mut *settings.as_mut_ptr();
                // Videos are decoded in parallel with each other: one thread per decoder avoids oversubscribing.
                s.n_threads = 1;
                s.max_frame_delay = 1;
                dav1d_open(
                    Some(NonNull::from(&mut ctx)),
                    Some(NonNull::from(&mut settings).cast()),
                )
            };
            anyhow::ensure!(r.0 == 0, "creating the AV1 decoder failed ({})", r.0);
            Ok(Self { ctx })
        }

        /// Hand one temporal unit to the decoder, taking out the pictures it finishes; false when it is rejected.
        fn send(&mut self, bytes: &[u8], f: &mut Plane) -> bool {
            use rav1d::{
                include::dav1d::data::Dav1dData,
                {dav1d_data_create, dav1d_data_unref, dav1d_send_data},
            };
            use std::{mem::MaybeUninit, ptr::NonNull};
            let mut data = MaybeUninit::<Dav1dData>::zeroed();
            // SAFETY: `data` is zeroed, an empty `Dav1dData`; `dav1d_data_create` then gives it a buffer of
            // `bytes.len()` writable bytes, and the copy stays inside it. `self.ctx` came from `dav1d_open` and
            // stays open until drop. `data` is unreferenced on every path out of the block.
            unsafe {
                let buf = dav1d_data_create(Some(NonNull::from(&mut data).cast()), bytes.len());
                if buf.is_null() {
                    return false;
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len());
                let mut ok = true;
                while (*data.as_ptr()).sz > 0 {
                    let r = dav1d_send_data(self.ctx, Some(NonNull::from(&mut data).cast()));
                    if r.0 < 0 && !would_block(r.0) {
                        ok = false;
                        break;
                    }
                    // The decoder keeps data it cannot take until its finished pictures are taken out.
                    self.pictures(f);
                }
                dav1d_data_unref(Some(NonNull::from(&mut data).cast()));
                self.pictures(f);
                ok
            }
        }

        fn drain(&mut self, f: &mut Plane) {
            self.pictures(f);
        }

        /// Pass the luma plane of every picture the decoder has ready to `f`.
        fn pictures(&mut self, f: &mut Plane) {
            use rav1d::{
                include::dav1d::picture::Dav1dPicture,
                {dav1d_get_picture, dav1d_picture_unref},
            };
            use std::{mem::MaybeUninit, ptr::NonNull};
            loop {
                let mut pic = MaybeUninit::<Dav1dPicture>::zeroed();
                // SAFETY: `pic` is zeroed and `dav1d_get_picture` overwrites it on success; then the luma
                // plane pointer is valid for `stride * h` bytes until `dav1d_picture_unref`, which runs before
                // `pic` goes out of scope.
                unsafe {
                    let r = dav1d_get_picture(self.ctx, Some(NonNull::from(&mut pic).cast()));
                    if r.0 != 0 {
                        return;
                    }
                    let p = &*pic.as_ptr();
                    let (w, h) = (p.p.w as usize, p.p.h as usize);
                    if let (Some(plane), true, 8) = (p.data[0], p.stride[0] > 0, p.p.bpc) {
                        let stride = p.stride[0] as usize;
                        f(
                            std::slice::from_raw_parts(plane.as_ptr().cast::<u8>(), stride * h),
                            stride,
                            w,
                            h,
                        );
                    }
                    dav1d_picture_unref(Some(NonNull::from(&mut pic).cast()));
                }
            }
        }
    }

    fn would_block(code: i32) -> bool {
        std::io::Error::from_raw_os_error(-code).kind() == std::io::ErrorKind::WouldBlock
    }

    impl Drop for Av1 {
        fn drop(&mut self) {
            // SAFETY: `self.ctx` came from `dav1d_open` and is closed exactly once, here.
            unsafe { rav1d::dav1d_close(Some(std::ptr::NonNull::from(&mut self.ctx))) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{asm_note, segments};

    #[test]
    fn asm_note_only_when_decoding_without_assembly() {
        assert!(asm_note(true, false).is_some_and(|n| n.contains("--features asm")));
        assert_eq!(asm_note(true, true), None);
        assert_eq!(asm_note(false, false), None);
    }

    fn spans(sync: &[usize], total: usize, target: usize) -> Vec<(usize, usize)> {
        segments(sync, total, target)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn segments_cut_at_sync_samples_once_long_enough() {
        // Sync every 2 samples, segments of at least 5: cuts land on 6, 12.
        let sync: Vec<usize> = (0..14).step_by(2).collect();
        assert_eq!(spans(&sync, 14, 5), [(0, 6), (6, 12), (12, 14)]);
    }

    #[test]
    fn segments_cover_every_sample_exactly_once() {
        let sync: Vec<usize> = (0..100).step_by(3).collect();
        for target in [1, 4, 10, 1000] {
            let s = segments(&sync, 100, target);
            assert_eq!(s.first().unwrap().start, 0);
            assert_eq!(s.last().unwrap().end, 100);
            assert!(s.windows(2).all(|w| w[0].end == w[1].start), "{s:?}");
        }
    }

    #[test]
    fn a_track_synced_only_at_its_start_is_one_segment() {
        assert_eq!(spans(&[0], 500, 10), [(0, 500)]);
    }

    #[test]
    fn an_empty_track_is_one_empty_segment() {
        assert_eq!(spans(&[0], 0, 10), [(0, 0)]);
    }
}
