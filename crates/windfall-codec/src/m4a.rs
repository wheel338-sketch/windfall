//! Bounded, deterministic AAC-LC/MP4 muxing. This module encodes no audio.
//! Adapter authors must resolve their encoder's distribution/patent terms.
//! Layout knowledge: ISO BMFF box registrations and FFmpeg's public movenc.c.
use crate::{AudioTags, CodecError, atomic::AtomicFile};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

pub const MAX_M4A_PACKETS: usize = 1_000_000;
pub const MAX_AAC_PACKET_BYTES: usize = 256 * 1024;
const FRAME_SAMPLES: u32 = 1024;
const RATES: &[u32] = &[
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

/// This muxer accepts AAC-LC, 1024-frame access units, mono or stereo.
/// SBR, PS, channel configuration zero, ADTS and encrypted packets are excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AacConfig {
    pub sample_rate: u32,
    pub channels: u16,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AacPacket {
    pub bytes: Vec<u8>,
}
/// Encoder delay and final padding, per channel, in decoded PCM frames.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AacTrim {
    pub delay: u32,
    pub padding: u32,
}

/// Adapter contract, deliberately independent of any encoder dependency.
/// PCM is interleaved at config's rate/count. Emit raw AAC access units in
/// decode order; flush all packets before returning trim from finish.
/// An adapter must bound buffering, reject nonfinite/incomplete PCM frames,
/// support cancellation in its owning worker, and preserve errors from emit.
pub trait AacEncoderAdapter {
    fn config(&self) -> AacConfig;
    fn encode(
        &mut self,
        pcm: &[f32],
        emit: &mut dyn FnMut(AacPacket) -> Result<(), CodecError>,
    ) -> Result<(), CodecError>;
    fn finish(
        &mut self,
        emit: &mut dyn FnMut(AacPacket) -> Result<(), CodecError>,
    ) -> Result<AacTrim, CodecError>;
}

#[derive(Debug)]
pub struct M4aWriter {
    file: AtomicFile,
    config: AacConfig,
    mdat_at: u32,
    bytes: u32,
    sizes: Vec<u32>,
    tags: Vec<u8>,
    broken: bool,
}
fn invalid(what: &str) -> CodecError {
    CodecError::InvalidInput(format!("M4A: {what}"))
}
fn be32(out: &mut Vec<u8>, n: u32) {
    out.extend_from_slice(&n.to_be_bytes());
}
fn be16(out: &mut Vec<u8>, n: u16) {
    out.extend_from_slice(&n.to_be_bytes());
}
fn atom(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 8);
    be32(&mut out, body.len() as u32 + 8);
    out.extend_from_slice(id);
    out.extend_from_slice(body);
    out
}
fn full(id: &[u8; 4], flags: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    be32(&mut out, flags);
    out.extend_from_slice(body);
    atom(id, &out)
}
fn children(id: &[u8; 4], boxes: &[Vec<u8>]) -> Vec<u8> {
    atom(id, &boxes.concat())
}
fn matrix(out: &mut Vec<u8>) {
    for v in [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000] {
        be32(out, v);
    }
}
fn handler(kind: &[u8; 4], name: &[u8]) -> Vec<u8> {
    let mut b = vec![0; 4];
    b.extend_from_slice(kind);
    b.extend_from_slice(&[0; 12]);
    b.extend_from_slice(name);
    b.push(0);
    full(b"hdlr", 0, &b)
}
fn tags(tags: &AudioTags<'_>) -> Result<Vec<u8>, CodecError> {
    let mut entries = Vec::new();
    for (key, text) in [
        (b"\xa9nam", tags.title),
        (b"\xa9ART", tags.author),
        (b"\xa9gen", tags.genre),
        (b"\xa9cmt", tags.comments),
    ] {
        if text.len() > 16384 || text.contains('\0') {
            return Err(invalid("tag exceeds 16384 bytes or contains NUL"));
        }
        if text.is_empty() {
            continue;
        }
        let mut data = Vec::new();
        be32(&mut data, 1);
        be32(&mut data, 0);
        data.extend_from_slice(text.as_bytes());
        entries.push(atom(key, &atom(b"data", &data)));
    }
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let mut meta = handler(b"mdir", b"Windfall");
    meta.extend(children(b"ilst", &entries));
    Ok(atom(b"udta", &full(b"meta", 0, &meta)))
}
impl M4aWriter {
    pub fn create(
        path: impl AsRef<Path>,
        config: AacConfig,
        metadata: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        // The version-zero sample entry has a 16.16 sample rate.
        if !RATES.contains(&config.sample_rate)
            || config.sample_rate > 65535
            || ![1, 2].contains(&config.channels)
        {
            return Err(invalid(
                "AAC-LC muxing requires a standard rate at most 64 kHz and one or two channels",
            ));
        }
        let tags = tags(metadata)?;
        let mut file = AtomicFile::create(path.as_ref())?;
        let ftyp = atom(b"ftyp", b"M4A \0\0\0\0M4A isommp42");
        let mdat_at = ftyp.len() as u32;
        file.write_all(&ftyp)?;
        file.write_all(&[0, 0, 0, 0, b'm', b'd', b'a', b't'])?;
        Ok(Self {
            file,
            config,
            mdat_at,
            bytes: 0,
            sizes: Vec::new(),
            tags,
            broken: false,
        })
    }
    /// Raw AAC, without an ADTS header. This validates framing/budgets, not
    /// AAC syntax; the adapter owns codec correctness and independent decoding.
    pub fn write_packet(&mut self, packet: &[u8]) -> Result<(), CodecError> {
        if self.broken {
            return Err(invalid("an earlier packet write failed"));
        }
        if packet.is_empty()
            || packet.len() > MAX_AAC_PACKET_BYTES
            || self.sizes.len() >= MAX_M4A_PACKETS
        {
            return Err(invalid("empty/oversized packet or packet budget exceeded"));
        }
        if packet.len() >= 2 && packet[0] == 0xff && packet[1] & 0xf6 == 0xf0 {
            return Err(invalid("ADTS headers must be removed by the adapter"));
        }
        let bytes = self
            .bytes
            .checked_add(packet.len() as u32)
            .filter(|n| *n <= u32::MAX - self.mdat_at - 8)
            .ok_or_else(|| invalid("32-bit MP4 media size exceeded"))?;
        self.broken = true;
        self.file.write_all(packet)?;
        self.broken = false;
        self.bytes = bytes;
        self.sizes.push(packet.len() as u32);
        Ok(())
    }
    /// Writes sample tables and a gapless edit list, then atomically publishes.
    /// Dropping or any failure leaves the prior destination intact.
    pub fn finalize(mut self, trim: AacTrim) -> Result<(), CodecError> {
        if self.broken {
            return Err(invalid("an earlier packet write failed"));
        }
        if self.sizes.is_empty() {
            return Err(CodecError::NoAudio);
        }
        let duration = (self.sizes.len() as u32)
            .checked_mul(FRAME_SAMPLES)
            .ok_or_else(|| invalid("media duration overflow"))?;
        let playable = duration
            .checked_sub(trim.delay)
            .and_then(|n| n.checked_sub(trim.padding))
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("delay/padding consume all media"))?;
        if trim.delay > i32::MAX as u32 {
            return Err(invalid("edit-list delay overflow"));
        }
        let moov = self.moov(duration, playable, trim.delay);
        if u64::from(self.mdat_at) + 8 + u64::from(self.bytes) + moov.len() as u64
            > u64::from(u32::MAX)
        {
            return Err(invalid("32-bit MP4 file size exceeded"));
        }
        self.file.write_all(&moov)?;
        let at = self.mdat_at;
        let size = self.bytes + 8;
        self.file.place(|file| {
            file.seek(SeekFrom::Start(u64::from(at)))?;
            file.write_all(&size.to_be_bytes())
        })?;
        Ok(())
    }
    fn moov(&self, duration: u32, playable: u32, delay: u32) -> Vec<u8> {
        let mut mv = vec![0; 8];
        be32(&mut mv, self.config.sample_rate);
        be32(&mut mv, playable);
        be32(&mut mv, 0x10000);
        be16(&mut mv, 0x100);
        mv.extend_from_slice(&[0; 10]);
        matrix(&mut mv);
        mv.extend_from_slice(&[0; 24]);
        be32(&mut mv, 2);
        let mut tk = vec![0; 8];
        be32(&mut tk, 1);
        be32(&mut tk, 0);
        be32(&mut tk, playable);
        tk.extend_from_slice(&[0; 8]);
        be16(&mut tk, 0);
        be16(&mut tk, 0);
        be16(&mut tk, 0x100);
        be16(&mut tk, 0);
        matrix(&mut tk);
        tk.extend_from_slice(&[0; 8]);
        let mut edit = Vec::new();
        be32(&mut edit, 1);
        be32(&mut edit, playable);
        be32(&mut edit, delay);
        be16(&mut edit, 1);
        be16(&mut edit, 0);
        let mut md = vec![0; 8];
        be32(&mut md, self.config.sample_rate);
        be32(&mut md, duration);
        be16(&mut md, 0x55c4);
        be16(&mut md, 0);
        let mut dref = Vec::new();
        be32(&mut dref, 1);
        dref.extend(full(b"url ", 1, &[]));
        let minf = children(
            b"minf",
            &[
                full(b"smhd", 0, &[0; 4]),
                atom(b"dinf", &full(b"dref", 0, &dref)),
                self.sample_table(),
            ],
        );
        let mdia = children(
            b"mdia",
            &[
                full(b"mdhd", 0, &md),
                handler(b"soun", b"Windfall audio"),
                minf,
            ],
        );
        let trak = children(
            b"trak",
            &[
                full(b"tkhd", 3, &tk),
                atom(b"edts", &full(b"elst", 0, &edit)),
                mdia,
            ],
        );
        let mut body = full(b"mvhd", 0, &mv);
        body.extend(trak);
        body.extend_from_slice(&self.tags);
        atom(b"moov", &body)
    }
    fn sample_table(&self) -> Vec<u8> {
        let index = RATES
            .iter()
            .position(|r| *r == self.config.sample_rate)
            .expect("validated config") as u16;
        let asc = (2_u16 << 11) | (index << 7) | (self.config.channels << 3);
        // ES_Descriptor / DecoderConfigDescriptor / DecoderSpecificInfo /
        // SLConfigDescriptor. These small descriptor sizes fit one VLQ byte.
        let mut decoder = vec![0x40, 0x15, 0x04, 0, 0];
        decoder.extend_from_slice(&[0; 8]);
        decoder.extend([5, 2]);
        decoder.extend_from_slice(&asc.to_be_bytes());
        let mut es = vec![0, 1, 0, 4, decoder.len() as u8];
        es.extend(decoder);
        es.extend([6, 1, 2]);
        let mut descriptor = vec![3, es.len() as u8];
        descriptor.extend(es);
        let mut entry = vec![0; 6];
        be16(&mut entry, 1);
        entry.extend_from_slice(&[0; 8]);
        be16(&mut entry, self.config.channels);
        be16(&mut entry, 16);
        entry.extend_from_slice(&[0; 4]);
        be32(&mut entry, self.config.sample_rate << 16);
        entry.extend(full(b"esds", 0, &descriptor));
        let mut stsd = Vec::new();
        be32(&mut stsd, 1);
        stsd.extend(atom(b"mp4a", &entry));
        let count = self.sizes.len() as u32;
        let mut stts = Vec::new();
        for v in [1, count, FRAME_SAMPLES] {
            be32(&mut stts, v);
        }
        let mut stsc = Vec::new();
        for v in [1, 1, count, 1] {
            be32(&mut stsc, v);
        }
        let mut stsz = Vec::new();
        be32(&mut stsz, 0);
        be32(&mut stsz, count);
        for size in &self.sizes {
            be32(&mut stsz, *size);
        }
        let mut stco = Vec::new();
        be32(&mut stco, 1);
        be32(&mut stco, self.mdat_at + 8);
        children(
            b"stbl",
            &[
                full(b"stsd", 0, &stsd),
                full(b"stts", 0, &stts),
                full(b"stsc", 0, &stsc),
                full(b"stsz", 0, &stsz),
                full(b"stco", 0, &stco),
            ],
        )
    }
}
