//! RIFF/WAVE cue, adtl/labl/ltxt, smpl and ACID metadata.
//! Positions use frames, never interleaved sample indices. Loop ends in this
//! API are exclusive; the smpl chunk stores an inclusive last frame.
//! Public layouts: Microsoft RIFF specification and libsndfile src/wav.c.
use crate::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;

pub const MAX_WAV_MARKERS: usize = 4096;
pub const MAX_WAV_METADATA_BYTES: usize = 4 * 1024 * 1024;
const MAX_LABEL_BYTES: usize = 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct WavMetadata {
    pub cues: Vec<WavCue>,
    pub loops: Vec<WavLoop>,
    /// Region lengths keyed by cue ID; slice starts are ordinary cue points.
    pub slices: Vec<WavSlice>,
    pub tempo: Option<WavTempo>,
    /// MIDI unity note for smpl. None writes the conventional C4 (60).
    pub root_key: Option<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WavCue {
    pub id: u32,
    pub frame: u32,
    pub label: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavLoopKind {
    Forward,
    Alternating,
    Backward,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavLoop {
    pub cue_id: u32,
    pub start: u32,
    pub end: u32,
    pub kind: WavLoopKind,
    /// Zero repeats indefinitely.
    pub play_count: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavSlice {
    pub cue_id: u32,
    pub length: u32,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WavTempo {
    pub bpm: f32,
    pub beats: u32,
    pub numerator: u16,
    pub denominator: u16,
    pub root_key: Option<u8>,
    pub one_shot: bool,
}

fn bad(message: &str) -> CodecError {
    CodecError::InvalidInput(format!("WAV metadata: {message}"))
}
fn corrupt(message: &str) -> CodecError {
    CodecError::Corrupt(format!("WAV metadata: {message}"))
}
fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().expect("checked chunk"))
}
fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().expect("checked chunk"))
}
fn word(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}
fn chunk(out: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(id);
    word(out, body.len() as u32);
    out.extend_from_slice(body);
    if body.len() % 2 != 0 {
        out.push(0);
    }
}
impl WavMetadata {
    pub fn validate(&self, frames: Option<u64>) -> Result<(), CodecError> {
        if self.cues.len() > MAX_WAV_MARKERS
            || self.loops.len() > MAX_WAV_MARKERS
            || self.slices.len() > MAX_WAV_MARKERS
        {
            return Err(bad("too many entries"));
        }
        let mut ids = BTreeMap::new();
        for cue in &self.cues {
            if ids.insert(cue.id, cue.frame).is_some() {
                return Err(bad("duplicate cue ID"));
            }
            if cue.label.len() > MAX_LABEL_BYTES || cue.label.contains('\0') {
                return Err(bad("label too long or contains NUL"));
            }
            if frames.is_some_and(|n| u64::from(cue.frame) > n) {
                return Err(bad("cue beyond audio"));
            }
        }
        if self.root_key.is_some_and(|key| key > 127) {
            return Err(bad("invalid root key"));
        }
        let mut loops = BTreeSet::new();
        for l in &self.loops {
            if l.start >= l.end
                || frames.is_some_and(|n| u64::from(l.end) > n)
                || !loops.insert(l.cue_id)
            {
                return Err(bad("invalid or duplicate loop"));
            }
        }
        let mut slices = BTreeSet::new();
        for slice in &self.slices {
            let start = ids
                .get(&slice.cue_id)
                .ok_or_else(|| bad("slice refers to missing cue"))?;
            let end = start
                .checked_add(slice.length)
                .ok_or_else(|| bad("slice end overflow"))?;
            if slice.length == 0
                || frames.is_some_and(|n| u64::from(end) > n)
                || !slices.insert(slice.cue_id)
            {
                return Err(bad("invalid or duplicate slice"));
            }
        }
        if let Some(t) = self.tempo
            && (!t.bpm.is_finite()
                || t.bpm <= 0.0
                || t.numerator == 0
                || ![1, 2, 4, 8, 16, 32, 64].contains(&t.denominator)
                || t.root_key.is_some_and(|key| key > 127))
        {
            return Err(bad("invalid tempo or meter"));
        }
        Ok(())
    }
    pub(crate) fn chunks(&self, rate: u32) -> Result<Vec<u8>, CodecError> {
        self.validate(None)?;
        if rate == 0 {
            return Err(bad("zero sample rate"));
        }
        let mut out = Vec::new();
        if !self.cues.is_empty() {
            let mut cues = Vec::new();
            word(&mut cues, self.cues.len() as u32);
            let mut adtl = b"adtl".to_vec();
            for cue in &self.cues {
                word(&mut cues, cue.id);
                word(&mut cues, cue.frame);
                cues.extend_from_slice(b"data");
                word(&mut cues, 0);
                word(&mut cues, 0);
                word(&mut cues, cue.frame);
                if !cue.label.is_empty() {
                    let mut label = Vec::new();
                    word(&mut label, cue.id);
                    label.extend_from_slice(cue.label.as_bytes());
                    label.push(0);
                    chunk(&mut adtl, b"labl", &label);
                }
            }
            for slice in &self.slices {
                let mut region = Vec::new();
                word(&mut region, slice.cue_id);
                word(&mut region, slice.length);
                region.extend_from_slice(b"rgn ");
                region.extend_from_slice(&[0; 8]);
                chunk(&mut adtl, b"ltxt", &region);
            }
            chunk(&mut out, b"cue ", &cues);
            if adtl.len() > 4 {
                chunk(&mut out, b"LIST", &adtl);
            }
        }
        if !self.loops.is_empty() || self.root_key.is_some() {
            let mut body = Vec::new();
            word(&mut body, 0);
            word(&mut body, 0);
            word(
                &mut body,
                (1_000_000_000.0 / f64::from(rate)).round() as u32,
            );
            word(&mut body, u32::from(self.root_key.unwrap_or(60)));
            for _ in 0..3 {
                word(&mut body, 0);
            }
            word(&mut body, self.loops.len() as u32);
            word(&mut body, 0);
            for l in &self.loops {
                word(&mut body, l.cue_id);
                word(
                    &mut body,
                    match l.kind {
                        WavLoopKind::Forward => 0,
                        WavLoopKind::Alternating => 1,
                        WavLoopKind::Backward => 2,
                    },
                );
                word(&mut body, l.start);
                word(&mut body, l.end - 1);
                word(&mut body, 0);
                word(&mut body, l.play_count);
            }
            chunk(&mut out, b"smpl", &body);
        }
        if let Some(t) = self.tempo {
            let mut body = Vec::new();
            word(
                &mut body,
                u32::from(t.one_shot) | if t.root_key.is_some() { 2 } else { 0 },
            );
            body.extend_from_slice(&u16::from(t.root_key.unwrap_or(0)).to_le_bytes());
            body.extend_from_slice(&0_u16.to_le_bytes());
            body.extend_from_slice(&0_f32.to_le_bytes());
            word(&mut body, t.beats);
            body.extend_from_slice(&t.denominator.to_le_bytes());
            body.extend_from_slice(&t.numerator.to_le_bytes());
            body.extend_from_slice(&t.bpm.to_le_bytes());
            chunk(&mut out, b"acid", &body);
        }
        if out.len() > MAX_WAV_METADATA_BYTES {
            return Err(bad("metadata byte budget exceeded"));
        }
        Ok(out)
    }
}

pub fn read_wav_metadata_file(path: impl AsRef<Path>) -> Result<WavMetadata, CodecError> {
    read(&mut File::open(path)?)
}
pub fn read_wav_metadata(bytes: &[u8]) -> Result<WavMetadata, CodecError> {
    read(&mut Cursor::new(bytes))
}

/// Seeks over audio and unknown chunks without allocating their declared size.
fn read<R: Read + Seek>(reader: &mut R) -> Result<WavMetadata, CodecError> {
    let length = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(0))?;
    let mut header = [0; 12];
    reader
        .read_exact(&mut header)
        .map_err(|_| corrupt("short RIFF header"))?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err(CodecError::UnsupportedFormat(
            "metadata needs RIFF/WAVE".into(),
        ));
    }
    let end = 8 + u64::from(u32le(&header, 4));
    if end > length || end < 12 {
        return Err(corrupt("RIFF size exceeds input"));
    }
    let mut result = WavMetadata::default();
    let mut labels = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut align = None;
    let mut data_bytes = None;
    let mut budget = 0;
    let mut at = 12;
    while at < end {
        if end - at < 8 {
            return Err(corrupt("truncated chunk header"));
        }
        let mut h = [0; 8];
        reader.seek(SeekFrom::Start(at))?;
        reader.read_exact(&mut h)?;
        let size = u32le(&h, 4) as usize;
        let next = at + 8 + size as u64 + (size % 2) as u64;
        if next > end {
            return Err(corrupt("chunk exceeds RIFF size"));
        }
        match &h[..4] {
            b"fmt " => {
                if !seen.insert(*b"fmt ") || !(16..=4096).contains(&size) {
                    return Err(corrupt("invalid fmt chunk"));
                }
                let mut b = vec![0; size];
                reader.read_exact(&mut b)?;
                let tag = u16le(&b, 0);
                let pcm = tag == 1
                    || tag == 3
                    || (tag == 0xfffe && size >= 40 && [1, 3].contains(&u16le(&b, 24)));
                if !pcm {
                    return Err(CodecError::UnsupportedFormat(
                        "metadata positions require PCM WAV".into(),
                    ));
                }
                let channels = u16le(&b, 2);
                let rate = u32le(&b, 4);
                let bits = u16le(&b, 14);
                let a = u16le(&b, 12);
                if channels == 0
                    || rate == 0
                    || ![8, 16, 24, 32, 64].contains(&bits)
                    || u32::from(a) != u32::from(channels) * u32::from(bits / 8)
                    || u64::from(u32le(&b, 8)) != u64::from(rate) * u64::from(a)
                {
                    return Err(corrupt("invalid PCM frame layout"));
                }
                if tag == 0xfffe
                    && (u16le(&b, 16) < 22
                        || b[26..40]
                            != [0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xaa, 0, 0x38, 0x9b, 0x71])
                {
                    return Err(CodecError::UnsupportedFormat(
                        "unknown extensible PCM subtype".into(),
                    ));
                }
                align = Some(u64::from(a));
            }
            b"data" => {
                if data_bytes.replace(size as u64).is_some() {
                    return Err(corrupt("multiple data chunks"));
                }
            }
            b"cue " | b"smpl" | b"acid" | b"LIST" => {
                budget += size;
                if budget > MAX_WAV_METADATA_BYTES {
                    return Err(corrupt("metadata byte budget exceeded"));
                }
                let mut b = vec![0; size];
                reader.read_exact(&mut b)?;
                match &h[..4] {
                    b"cue " => {
                        if !seen.insert(*b"cue ") || b.len() < 4 {
                            return Err(corrupt("invalid cue chunk"));
                        }
                        let n = u32le(&b, 0) as usize;
                        if n > MAX_WAV_MARKERS || b.len() != 4 + n * 24 {
                            return Err(corrupt("invalid cue count"));
                        }
                        for c in b[4..].chunks_exact(24) {
                            if &c[8..12] != b"data" || u32le(c, 12) != 0 || u32le(c, 16) != 0 {
                                return Err(CodecError::UnsupportedFormat(
                                    "non-data cue positions".into(),
                                ));
                            }
                            result.cues.push(WavCue {
                                id: u32le(c, 0),
                                frame: u32le(c, 20),
                                label: String::new(),
                            });
                        }
                    }
                    b"smpl" => {
                        if !seen.insert(*b"smpl") || b.len() < 36 {
                            return Err(corrupt("invalid smpl chunk"));
                        }
                        let n = u32le(&b, 28) as usize;
                        if n > MAX_WAV_MARKERS || b.len() < 36 + n * 24 {
                            return Err(corrupt("invalid loop count"));
                        }
                        if u32le(&b, 12) > 127 {
                            return Err(corrupt("invalid root key"));
                        }
                        result.root_key = Some(u32le(&b, 12) as u8);
                        if u32le(&b, 16) != 0 {
                            return Err(CodecError::UnsupportedFormat(
                                "fractional smpl tuning".into(),
                            ));
                        }
                        for l in b[36..36 + n * 24].chunks_exact(24) {
                            let kind = match u32le(l, 4) {
                                0 => WavLoopKind::Forward,
                                1 => WavLoopKind::Alternating,
                                2 => WavLoopKind::Backward,
                                _ => {
                                    return Err(CodecError::UnsupportedFormat(
                                        "smpl loop type".into(),
                                    ));
                                }
                            };
                            if u32le(l, 16) != 0 {
                                return Err(CodecError::UnsupportedFormat(
                                    "fractional loop bounds".into(),
                                ));
                            }
                            result.loops.push(WavLoop {
                                cue_id: u32le(l, 0),
                                start: u32le(l, 8),
                                end: u32le(l, 12)
                                    .checked_add(1)
                                    .ok_or_else(|| corrupt("loop end overflow"))?,
                                kind,
                                play_count: u32le(l, 20),
                            });
                        }
                    }
                    b"acid" => {
                        if !seen.insert(*b"acid") || b.len() != 24 {
                            return Err(corrupt("invalid ACID chunk"));
                        }
                        let root = u16le(&b, 4);
                        if u32le(&b, 0) & 2 != 0 && root > 127 {
                            return Err(corrupt("invalid ACID root"));
                        }
                        result.tempo = Some(WavTempo {
                            bpm: f32::from_le_bytes(b[20..24].try_into().expect("checked size")),
                            beats: u32le(&b, 12),
                            denominator: u16le(&b, 16),
                            numerator: u16le(&b, 18),
                            root_key: (u32le(&b, 0) & 2 != 0).then_some(root as u8),
                            one_shot: u32le(&b, 0) & 1 != 0,
                        });
                    }
                    b"LIST" if b.starts_with(b"adtl") => {
                        let mut pos = 4;
                        while pos < b.len() {
                            if b.len() - pos < 8 {
                                return Err(corrupt("short adtl header"));
                            }
                            let n = u32le(&b, pos + 4) as usize;
                            let stop = pos
                                .checked_add(8 + n)
                                .ok_or_else(|| corrupt("adtl overflow"))?;
                            if stop + n % 2 > b.len() {
                                return Err(corrupt("adtl subchunk exceeds list"));
                            }
                            let data = &b[pos + 8..stop];
                            match &b[pos..pos + 4] {
                                b"labl" => {
                                    if n < 5 || n > MAX_LABEL_BYTES + 5 || data[n - 1] != 0 {
                                        return Err(corrupt("invalid label"));
                                    }
                                    let label =
                                        std::str::from_utf8(&data[4..n - 1]).map_err(|_| {
                                            CodecError::UnsupportedFormat(
                                                "non-UTF8 cue label".into(),
                                            )
                                        })?;
                                    if labels.insert(u32le(data, 0), label.to_owned()).is_some()
                                        || labels.len() > MAX_WAV_MARKERS
                                    {
                                        return Err(corrupt("duplicate or excess labels"));
                                    }
                                }
                                b"ltxt" if n >= 20 && &data[8..12] == b"rgn " => {
                                    if result.slices.len() >= MAX_WAV_MARKERS {
                                        return Err(corrupt("too many regions"));
                                    }
                                    result.slices.push(WavSlice {
                                        cue_id: u32le(data, 0),
                                        length: u32le(data, 4),
                                    });
                                }
                                _ => {}
                            }
                            pos = stop + n % 2;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        at = next;
    }
    for cue in &mut result.cues {
        cue.label = labels.remove(&cue.id).unwrap_or_default();
    }
    if !labels.is_empty() {
        return Err(corrupt("label refers to missing cue"));
    }
    let (Some(bytes), Some(align)) = (data_bytes, align) else {
        return Err(corrupt("missing fmt or data"));
    };
    if bytes % align != 0 {
        return Err(corrupt("partial audio frame"));
    }
    result
        .validate(Some(bytes / align))
        .map_err(|e| corrupt(&e.to_string()))?;
    Ok(result)
}
