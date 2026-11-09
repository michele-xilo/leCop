//! Shared test helpers: a minimal mp4 writer. Only the boxes the parser reads are emitted, and no
//! frame data, so fixtures are a few hundred bytes.
#![allow(dead_code)]
pub mod synth;

fn bx(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut v = ((payload.len() + 8) as u32).to_be_bytes().to_vec();
    v.extend(kind);
    v.extend(payload);
    v
}

fn be(x: u32) -> [u8; 4] {
    x.to_be_bytes()
}

/// `stsd` with one VisualSampleEntry of the given size, laid out like a real AV1 or H.264 entry.
fn stsd(width: u16, height: u16) -> Vec<u8> {
    let mut e = vec![0u8; 6]; // reserved
    e.extend([0, 1]); // data_reference_index
    e.extend([0u8; 16]); // pre_defined, reserved, pre_defined
    e.extend(width.to_be_bytes());
    e.extend(height.to_be_bytes());
    e.extend(vec![0u8; 50]); // resolution, frame count, compressor name, depth, pre_defined
    let mut v = vec![0u8; 4];
    v.extend(be(1)); // entry count
    v.extend(bx(b"av01", &e));
    v
}

fn trak(
    handler: &[u8; 4],
    timescale: u32,
    stts: &[(u32, u32)],
    samples: u32,
    size: Option<(u16, u16)>,
) -> Vec<u8> {
    let mut mdhd = vec![0u8; 4]; // version + flags
    mdhd.extend([0u8; 8]); // creation, modification
    mdhd.extend(be(timescale));
    mdhd.extend(be(stts.iter().map(|(c, d)| c * d).sum()));
    mdhd.extend([0u8; 4]); // language, pre_defined

    let mut hdlr = vec![0u8; 8]; // version/flags, pre_defined
    hdlr.extend(handler);
    hdlr.extend([0u8; 12]);
    hdlr.push(0); // empty name

    let mut st = vec![0u8; 4];
    st.extend(be(stts.len() as u32));
    for (c, d) in stts {
        st.extend(be(*c));
        st.extend(be(*d));
    }

    // sample_size = 0 means a per-sample table follows, like real files
    let mut sz = vec![0u8; 4];
    sz.extend(be(0));
    sz.extend(be(samples));
    for _ in 0..samples {
        sz.extend(be(100));
    }

    let mut tables = vec![bx(b"stts", &st), bx(b"stsz", &sz)];
    if let Some((w, h)) = size {
        tables.insert(0, bx(b"stsd", &stsd(w, h)));
    }
    let stbl = bx(b"stbl", &tables.concat());
    let minf = bx(b"minf", &stbl);
    let mdia = bx(
        b"mdia",
        &[bx(b"mdhd", &mdhd), bx(b"hdlr", &hdlr), minf].concat(),
    );
    bx(b"trak", &mdia)
}

pub struct Mp4 {
    pub timescale: u32,
    /// (sample_count, sample_delta) runs, like the `stts` box
    pub stts: Vec<(u32, u32)>,
    pub moov_first: bool,
    pub audio_track_first: bool,
    pub video_track: bool,
    /// Width and height written to the `stsd` sample entry; no `stsd` box when None.
    pub size: Option<(u16, u16)>,
    pub large_mdat: bool,
    /// Size of the (zero-filled) frame data box.
    pub mdat_len: usize,
}

impl Mp4 {
    /// `frames` frames at `fps`, laid out like ffmpeg output (moov after mdat).
    pub fn constant_fps(frames: u32, fps: u32) -> Self {
        Mp4 {
            timescale: fps * 1024,
            stts: vec![(frames, 1024)],
            moov_first: false,
            audio_track_first: false,
            video_track: true,
            size: None,
            large_mdat: false,
            mdat_len: 64,
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        let frames: u32 = self.stts.iter().map(|(c, _)| c).sum();
        let mut tracks = Vec::new();
        if self.audio_track_first {
            tracks.push(trak(b"soun", 48000, &[(50, 1024)], 50, None));
        }
        if self.video_track {
            tracks.push(trak(b"vide", self.timescale, &self.stts, frames, self.size));
        }
        let moov = bx(b"moov", &tracks.concat());

        let ftyp = bx(b"ftyp", b"isom\0\0\0\0isom");
        let mdat = if self.large_mdat {
            // 64-bit "largesize" form: size field == 1, real size after the type
            let payload = vec![0u8; 64];
            let mut v = be(1).to_vec();
            v.extend(b"mdat");
            v.extend((payload.len() as u64 + 16).to_be_bytes());
            v.extend(payload);
            v
        } else {
            bx(b"mdat", &vec![0u8; self.mdat_len])
        };
        if self.moov_first {
            [ftyp, moov, mdat].concat()
        } else {
            [ftyp, mdat, moov].concat()
        }
    }
}
