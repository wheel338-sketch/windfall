//! Independent PCM implementation of the public WavPack 4/5 block format.
//! Framing: https://www.wavpack.com/WavPack5FileFormat.pdf
//! Predictors / Golomb coding: https://www.wavpack.com/WavPack.pdf (Bryant).
//! Only lossless integer mono/stereo is supported. No native codec is loaded.
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{AudioInfo, AudioTags, CodecError, DecodeOptions, atomic::AtomicFile};
use windfall_core::AudioBuffer;

const RATES: [u32; 16] = [
    6000, 8000, 9600, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000, 64000, 88200, 96000,
    192000, 0,
];
const MAX_BLOCK_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCK_FRAMES: usize = 1_048_576;
pub const DEFAULT_WAVPACK_BLOCK_FRAMES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavPackDepth {
    Int16,
    Int24,
}
impl WavPackDepth {
    fn bits(self) -> u32 {
        match self {
            Self::Int16 => 16,
            Self::Int24 => 24,
        }
    }
}
fn bad(reason: &str) -> CodecError {
    CodecError::Corrupt(format!("WavPack: {reason}"))
}
fn unsupported(reason: &str) -> CodecError {
    CodecError::UnsupportedFormat(format!("WavPack: {reason}"))
}
pub(crate) fn check_layout(rate: u32, channels: u16) -> Result<(), CodecError> {
    if rate == 0 || rate > 0xff_ffff || ![1, 2].contains(&channels) {
        return Err(CodecError::InvalidInput(
            "WavPack requires mono/stereo and a rate in 1..16777215 Hz".into(),
        ));
    }
    Ok(())
}
fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}
impl Bits<'_> {
    fn get(&mut self, n: u32) -> Result<u32, CodecError> {
        if n > 32
            || self
                .at
                .checked_add(n as usize)
                .is_none_or(|end| end > self.data.len() * 8)
        {
            return Err(bad("truncated bitstream"));
        }
        let mut v = 0;
        for shift in 0..n {
            v |= u32::from((self.data[self.at / 8] >> (self.at % 8)) & 1) << shift;
            self.at += 1;
        }
        Ok(v)
    }
    fn unary(&mut self, limit: u32) -> Result<u32, CodecError> {
        for n in 0..=limit {
            if self.get(1)? == 0 {
                return Ok(n);
            }
        }
        Err(bad("unbounded unary code"))
    }
    fn gamma(&mut self) -> Result<u32, CodecError> {
        let n = self.unary(32)?;
        Ok(if n < 2 {
            n
        } else {
            self.get(n - 1)? | (1 << (n - 1))
        })
    }
    fn truncated(&mut self, max: u32) -> Result<u32, CodecError> {
        if max == 0 {
            return Ok(0);
        }
        let n = 32 - max.leading_zeros();
        let extras = ((1_u64 << n) - u64::from(max) - 1) as u32;
        let v = self.get(n - 1)?;
        Ok(if v < extras {
            v
        } else {
            (v * 2) - extras + self.get(1)?
        })
    }
}
#[derive(Default)]
struct PutBits {
    bytes: Vec<u8>,
    at: usize,
}
impl PutBits {
    fn put(&mut self, v: u32, n: u32) {
        for shift in 0..n {
            if self.at / 8 == self.bytes.len() {
                self.bytes.push(0);
            }
            self.bytes[self.at / 8] |= (((v >> shift) & 1) as u8) << (self.at % 8);
            self.at += 1;
        }
    }
    fn unary(&mut self, n: u32) {
        for _ in 0..n {
            self.put(1, 1);
        }
        self.put(0, 1);
    }
    fn gamma(&mut self, v: u32) {
        let n = 32 - v.leading_zeros();
        self.unary(n);
        if n > 1 {
            self.put(v, n - 1);
        }
    }
    fn truncated(&mut self, v: u32, max: u32) {
        if max == 0 {
            return;
        }
        let n = 32 - max.leading_zeros();
        let extras = ((1_u64 << n) - u64::from(max) - 1) as u32;
        if v < extras {
            self.put(v, n - 1);
        } else {
            let v = v + extras;
            self.put(v >> 1, n - 1);
            self.put(v & 1, 1);
        }
    }
    fn finish(mut self) -> Vec<u8> {
        while !self.at.is_multiple_of(16) {
            self.put(1, 1);
        }
        self.bytes
    }
}

// Metadata uses an 8-bit fractional binary logarithm, with a rounded 9-bit
// mantissa. Calculate the specified lookup mathematically rather than shipping
// an implementation's table. The smallest fractional boundary is far from a
// floating-point rounding tie.
fn exp(log: i16) -> Result<i32, CodecError> {
    let magnitude = i32::from(log).unsigned_abs();
    if magnitude > 8192 {
        return Err(bad("entropy/history logarithm out of range"));
    }
    let mantissa = (256.0 * 2_f64.powf(f64::from(magnitude & 255) / 256.0)).round() as u64;
    let exponent = magnitude >> 8;
    let v = if exponent <= 9 {
        mantissa >> (9 - exponent)
    } else {
        mantissa << (exponent - 9)
    };
    if v > i32::MAX as u64 {
        return Err(bad("entropy/history value out of range"));
    }
    Ok(if log < 0 { -(v as i32) } else { v as i32 })
}
fn adapt(m: &mut u32, index: usize, up: bool) -> Result<(), CodecError> {
    let divisor = [128_u64, 64, 32][index];
    let v = u64::from(*m);
    let next = if up {
        v + ((v + divisor) / divisor) * 5
    } else {
        v - ((v + divisor - 2) / divisor) * 2
    };
    *m = u32::try_from(next).map_err(|_| bad("entropy median overflow"))?;
    Ok(())
}
fn interval(m: &mut [u32; 3], zone: u32) -> Result<(u32, u32), CodecError> {
    let widths = m.map(|v| (v >> 4) + 1);
    let (low, width) = match zone {
        0 => (0_u64, widths[0]),
        1 => (u64::from(widths[0]), widths[1]),
        _ => (
            u64::from(widths[0])
                + u64::from(widths[1])
                + u64::from(zone - 2) * u64::from(widths[2]),
            widths[2],
        ),
    };
    for (index, value) in m.iter_mut().enumerate() {
        if zone < index as u32 {
            break;
        }
        adapt(value, index, zone > index as u32)?;
    }
    if low + u64::from(width) - 1 > i32::MAX as u64 {
        return Err(bad("residual range overflow"));
    }
    Ok((low as u32, width - 1))
}
fn zone(m: &[u32; 3], value: u32) -> u32 {
    let [a, b, c] = m.map(|v| (v >> 4) + 1);
    if value < a {
        0
    } else if value - a < b {
        1
    } else {
        2 + (value - a - b) / c
    }
}
fn entropy_decode(
    data: &[u8],
    count: usize,
    channels: usize,
    mut med: [[u32; 3]; 2],
) -> Result<Vec<i32>, CodecError> {
    let mut bits = Bits { data, at: 0 };
    let mut out = Vec::new();
    out.try_reserve_exact(count)
        .map_err(|_| CodecError::OutOfMemory)?;
    let (mut held_one, mut held_zero, mut zeros) = (0_u32, false, 0_u32);
    for i in 0..count {
        if med[0][0] < 2 && med[1][0] < 2 && !held_zero && held_one == 0 {
            if zeros > 0 {
                zeros -= 1;
            } else {
                zeros = bits.gamma()?;
                if zeros as usize > count - i {
                    return Err(bad("zero run exceeds block"));
                }
                if zeros > 0 {
                    med = [[0; 3]; 2];
                }
            }
            if zeros > 0 {
                out.push(0);
                continue;
            }
        }
        let z = if held_zero {
            held_zero = false;
            0
        } else {
            let mut raw = bits.unary(16)?;
            if raw == 16 {
                raw = raw
                    .checked_add(bits.gamma()?)
                    .ok_or_else(|| bad("unary overflow"))?;
            }
            let z = (raw >> 1) + held_one;
            held_one = raw & 1;
            held_zero = held_one == 0;
            z
        };
        let (low, max) = interval(&mut med[i % channels], z)?;
        let v = low + bits.truncated(max)?;
        out.push(if bits.get(1)? != 0 {
            !(v as i32)
        } else {
            v as i32
        });
    }
    Ok(out)
}
fn entropy_encode(
    samples: &[i32],
    channels: usize,
    initial: [[u32; 3]; 2],
) -> Result<Vec<u8>, CodecError> {
    // Plan prefixes before emitting them: the parity of each unary prefix
    // announces whether the following symbol has a zero prefix. This avoids
    // mutable pending-bit state and is bounded by one small PCM block.
    let mut med = initial;
    let mut symbols = Vec::with_capacity(samples.len());
    for (i, &s) in samples.iter().enumerate() {
        let value = if s < 0 { !s as u32 } else { s as u32 };
        let z = zone(&med[i % channels], value);
        let tiny = med[0][0] < 2 && med[1][0] < 2;
        let (low, max) = interval(&mut med[i % channels], z)?;
        symbols.push((z, value - low, max, s < 0, tiny));
    }
    let mut bits = PutBits::default();
    let (mut held_one, mut held_zero) = (0, false);
    for (i, &(z, rem, max, sign, tiny)) in symbols.iter().enumerate() {
        if tiny && !held_zero && held_one == 0 {
            bits.gamma(0);
        }
        if held_zero {
            held_zero = false;
        } else {
            let next_nonzero = symbols.get(i + 1).is_some_and(|s| s.0 != 0);
            let raw = (z - held_one) * 2 + u32::from(next_nonzero);
            bits.unary(raw.min(16));
            if raw >= 16 {
                bits.gamma(raw - 16);
            }
            held_one = raw & 1;
            held_zero = held_one == 0;
        }
        bits.truncated(rem, max);
        bits.put(u32::from(sign), 1);
    }
    Ok(bits.finish())
}

#[derive(Clone, Default)]
struct Pass {
    term: i32,
    delta: i32,
    weights: [i32; 2],
    history: [[i32; 8]; 2],
    cursor: usize,
}
fn prediction(weight: i32, source: i32) -> i32 {
    ((i64::from(weight) * i64::from(source) + 512) >> 10) as i32
}
fn update(weight: &mut i32, source: i32, residual: i32, delta: i32, clip: bool) {
    if source != 0 && residual != 0 {
        *weight = weight.wrapping_add(if (source ^ residual) < 0 {
            -delta
        } else {
            delta
        });
        if clip {
            *weight = (*weight).clamp(-1024, 1024);
        }
    }
}
impl Pass {
    fn source(&self, channel: usize) -> i32 {
        let h = self.history[channel];
        match self.term {
            17 => h[0].wrapping_mul(2).wrapping_sub(h[1]),
            18 => h[0].wrapping_mul(3).wrapping_sub(h[1]) >> 1,
            _ => h[self.cursor],
        }
    }
    fn positive(&mut self, pair: &mut [i32], encode: bool) {
        for (ch, s) in pair.iter_mut().enumerate() {
            let source = self.source(ch);
            let (value, residual) = if encode {
                (*s, s.wrapping_sub(prediction(self.weights[ch], source)))
            } else {
                (s.wrapping_add(prediction(self.weights[ch], source)), *s)
            };
            update(&mut self.weights[ch], source, residual, self.delta, false);
            if self.term > 8 {
                self.history[ch][1] = self.history[ch][0];
                self.history[ch][0] = value;
            } else {
                self.history[ch][(self.cursor + self.term as usize) & 7] = value;
            }
            *s = if encode { residual } else { value };
        }
        self.cursor = (self.cursor + 1) & 7;
    }
    fn decode(&mut self, pair: &mut [i32]) {
        if self.term > 0 {
            self.positive(pair, false);
            return;
        }
        let [ra, rb] = [pair[0], pair[1]];
        let (sa, sb, a, b) = match self.term {
            -1 => {
                let a = ra.wrapping_add(prediction(self.weights[0], self.history[0][0]));
                let b = rb.wrapping_add(prediction(self.weights[1], a));
                (self.history[0][0], a, a, b)
            }
            -2 => {
                let b = rb.wrapping_add(prediction(self.weights[1], self.history[1][0]));
                let a = ra.wrapping_add(prediction(self.weights[0], b));
                (b, self.history[1][0], a, b)
            }
            _ => {
                let a = ra.wrapping_add(prediction(self.weights[0], self.history[0][0]));
                let b = rb.wrapping_add(prediction(self.weights[1], self.history[1][0]));
                (self.history[0][0], self.history[1][0], a, b)
            }
        };
        update(&mut self.weights[0], sa, ra, self.delta, true);
        update(&mut self.weights[1], sb, rb, self.delta, true);
        self.history[0][0] = b;
        self.history[1][0] = a;
        pair[0] = a;
        pair[1] = b;
    }
}

type MetadataEntry<'a> = (u8, &'a [u8], usize);
fn metadata(block: &[u8]) -> Result<Vec<MetadataEntry<'_>>, CodecError> {
    let mut result = Vec::new();
    let mut at = 32;
    while at < block.len() {
        if result.len() >= 256 || block.len() - at < 2 {
            return Err(bad("invalid metadata count/header"));
        }
        let start = at;
        let id = block[at];
        let mut words = usize::from(block[at + 1]);
        at += 2;
        if id & 0x80 != 0 {
            if block.len() - at < 2 {
                return Err(bad("truncated large metadata"));
            }
            words |= usize::from(block[at]) << 8 | usize::from(block[at + 1]) << 16;
            at += 2;
        }
        let padded = words * 2;
        let length = padded
            .checked_sub(usize::from(id & 0x40 != 0))
            .ok_or_else(|| bad("odd empty metadata"))?;
        if padded > block.len() - at {
            return Err(bad("metadata exceeds block"));
        }
        result.push((id & 0x3f, &block[at..at + length], start));
        at += padded;
    }
    Ok(result)
}
fn header(block: &[u8]) -> Result<(AudioInfo, usize, u32), CodecError> {
    if block.len() < 32 || &block[..4] != b"wvpk" || u32le(block, 4) as usize + 8 != block.len() {
        return Err(bad("invalid block size/signature"));
    }
    let version = u16::from_le_bytes([block[8], block[9]]);
    if !(0x402..=0x410).contains(&version) {
        return Err(unsupported("only version 4/5 blocks are supported"));
    }
    if block[10] != 0 || block[11] != 0 {
        return Err(unsupported("40-bit sample indices are not supported"));
    }
    let flags = u32le(block, 24);
    if flags & 8 != 0 {
        return Err(unsupported(
            "hybrid/lossy audio is not supported; use lossless PCM",
        ));
    }
    if flags & 0x8000_0000 != 0 {
        return Err(unsupported("DSD is not supported"));
    }
    if flags & 0x80 != 0 {
        return Err(unsupported(
            "floating-point WavPack is not supported; use integer PCM",
        ));
    }
    if flags & 0x1800 != 0x1800 {
        return Err(unsupported(
            "multichannel block sequences are not supported",
        ));
    }
    let bits = ((flags & 3) + 1) * 8;
    if bits > 24 {
        return Err(unsupported(
            "integer samples wider than 24 bits are not supported",
        ));
    }
    let channels = if flags & 4 != 0 { 1 } else { 2 };
    let coded = if flags & (4 | 0x4000_0000) != 0 { 1 } else { 2 };
    let count = u32le(block, 20) as usize;
    if count > MAX_BLOCK_FRAMES {
        return Err(bad("block frame budget exceeded"));
    }
    let mut rate = RATES[((flags >> 23) & 15) as usize];
    let meta = metadata(block)?;
    for (id, data, _) in &meta {
        if *id == 0x27 {
            if data.len() != 3 && data.len() != 4 {
                return Err(bad("invalid sample-rate metadata"));
            }
            rate = u32::from(data[0]) | u32::from(data[1]) << 8 | u32::from(data[2]) << 16;
            if data.len() == 4 {
                rate |= u32::from(data[3]) << 24;
            }
        }
    }
    if rate == 0 || rate > 0xff_ffff {
        return Err(bad("invalid or missing sample rate"));
    }
    let total = u32le(block, 12);
    Ok((
        AudioInfo {
            sample_rate: rate,
            channels,
            frames: (total != u32::MAX).then_some(u64::from(total)),
            duration_secs: (total != u32::MAX).then_some(f64::from(total) / f64::from(rate)),
            format: "wavpack".into(),
            codec: "wavpack".into(),
            bits_per_sample: Some(bits),
        },
        coded,
        flags,
    ))
}
fn decode_block(block: &[u8], coded: usize, flags: u32) -> Result<Vec<i32>, CodecError> {
    let meta = metadata(block)?;
    let mut seen = [false; 64];
    let mut passes: Vec<Pass> = Vec::new();
    let mut med = [[0; 3]; 2];
    let mut stream = None;
    let mut integer = [0_u8; 4];
    let mut checksum = false;
    for (id, data, at) in meta {
        if id != 0 && seen[id as usize] {
            return Err(bad("duplicate metadata"));
        }
        seen[id as usize] = true;
        match id {
            0 => (),
            2 => {
                if data.len() > 16 {
                    return Err(bad("too many decorrelation passes"));
                }
                for &v in data {
                    let term = i32::from(v & 31) - 5;
                    if !matches!(term, -3..=-1 | 1..=8 | 17..=18) || (coded == 1 && term < 0) {
                        return Err(bad("invalid decorrelation term"));
                    }
                    passes.push(Pass {
                        term,
                        delta: i32::from(v >> 5),
                        ..Default::default()
                    });
                }
            }
            3 => {
                if data.len() % coded != 0 || data.len() / coded > passes.len() {
                    return Err(bad("invalid decorrelation weights"));
                }
                for (pass, weights) in passes.iter_mut().zip(data.chunks_exact(coded)) {
                    for (ch, &w) in weights.iter().enumerate() {
                        let w = i32::from(w as i8) * 8;
                        pass.weights[ch] = w + if w > 0 { (w + 64) >> 7 } else { 0 };
                    }
                }
            }
            4 => {
                let mut at = 0;
                for p in &mut passes {
                    if at == data.len() {
                        break;
                    }
                    let mut get = || -> Result<i32, CodecError> {
                        if data.len() - at < 2 {
                            return Err(bad("truncated decorrelation history"));
                        }
                        let n = exp(i16::from_le_bytes([data[at], data[at + 1]]))?;
                        at += 2;
                        Ok(n)
                    };
                    if p.term > 8 {
                        for ch in 0..coded {
                            p.history[ch][0] = get()?;
                            p.history[ch][1] = get()?;
                        }
                    } else if p.term < 0 {
                        p.history[0][0] = get()?;
                        p.history[1][0] = get()?;
                    } else {
                        for i in 0..p.term as usize {
                            for ch in 0..coded {
                                p.history[ch][i] = get()?;
                            }
                        }
                    }
                }
                if at != data.len() {
                    return Err(bad("excess decorrelation history"));
                }
            }
            5 => {
                if data.len() != coded * 6 {
                    return Err(bad("invalid entropy variables"));
                }
                for (ch, values) in data.as_chunks::<6>().0.iter().enumerate() {
                    for i in 0..3 {
                        let log = i16::from_le_bytes([values[i * 2], values[i * 2 + 1]]);
                        if log < 0 {
                            return Err(bad("negative entropy median"));
                        }
                        med[ch][i] = exp(log)? as u32;
                    }
                }
            }
            9 => {
                if data.len() != 4 {
                    return Err(bad("invalid integer metadata"));
                }
                integer.copy_from_slice(data);
                if integer[0] != 0 {
                    return Err(unsupported(
                        "extended integer side streams are not supported",
                    ));
                }
                if integer[1..].iter().filter(|v| **v != 0).count() > 1
                    || integer.iter().any(|v| *v > 24)
                {
                    return Err(bad("invalid integer restoration"));
                }
            }
            10 => stream = Some(data),
            13 => {
                let output_channels = if flags & 4 != 0 { 1 } else { 2 };
                if data.is_empty() || data.len() > 5 || usize::from(data[0]) + 1 != output_channels
                {
                    return Err(unsupported(
                        "channel metadata does not describe mono/stereo PCM",
                    ));
                }
            }
            0x2f => {
                if ![2, 4].contains(&data.len()) || at + data.len() + 2 != block.len() {
                    return Err(bad("invalid block checksum"));
                }
                let mut sum = u32::MAX;
                for word in block[..at].as_chunks::<2>().0 {
                    sum = sum
                        .wrapping_mul(3)
                        .wrapping_add(u32::from(u16::from_le_bytes([word[0], word[1]])));
                }
                if data.len() == 2 {
                    sum = (sum ^ (sum >> 16)) & 0xffff;
                }
                let stored = if data.len() == 2 {
                    u32::from(u16::from_le_bytes([data[0], data[1]]))
                } else {
                    u32le(data, 0)
                };
                if sum != stored {
                    return Err(bad("block checksum mismatch"));
                }
                checksum = true;
            }
            _ if id & 0x20 != 0 => (),
            _ => return Err(unsupported("unrecognized required metadata")),
        }
    }
    if flags & 0x1000_0000 != 0 && !checksum {
        return Err(bad("missing block checksum"));
    }
    let frames = u32le(block, 20) as usize;
    if frames == 0 {
        return Ok(Vec::new());
    }
    if !seen[5] {
        return Err(bad("missing entropy variables"));
    }
    let mut samples = entropy_decode(
        stream.ok_or_else(|| bad("missing audio bitstream"))?,
        frames * coded,
        coded,
        med,
    )?;
    for pair in samples.chunks_exact_mut(coded) {
        for pass in passes.iter_mut().rev() {
            pass.decode(pair);
        }
        if flags & 16 != 0 && coded == 2 {
            let right = pair[1].wrapping_sub(pair[0] >> 1);
            pair[0] = pair[0].wrapping_add(right);
            pair[1] = right;
        }
    }
    let crc = samples.iter().fold(u32::MAX, |crc, &v| {
        crc.wrapping_mul(3).wrapping_add(v as u32)
    });
    if crc != u32le(block, 28) {
        return Err(bad("PCM CRC mismatch"));
    }
    let shift = (flags >> 13) & 31;
    let width = ((flags & 3) + 1) * 8;
    for v in &mut samples {
        let mut value = i64::from(*v);
        if flags & 0x100 != 0 {
            let [_, zeros, ones, dups] = integer;
            value = if zeros != 0 {
                value << zeros
            } else if ones != 0 {
                ((value + 1) << ones) - 1
            } else if dups != 0 {
                ((value + (value & 1)) << dups) - (value & 1)
            } else {
                value
            };
        }
        value = value
            .checked_mul(1_i64 << shift)
            .ok_or_else(|| bad("sample shift overflow"))?;
        if value < -(1_i64 << (width - 1)) || value >= (1_i64 << (width - 1)) {
            return Err(bad("sample exceeds declared bit depth"));
        }
        *v = value as i32;
    }
    Ok(samples)
}
fn read_block(reader: &mut impl Read) -> Result<Option<Vec<u8>>, CodecError> {
    let mut h = [0; 32];
    if reader.read(&mut h[..1])? == 0 {
        return Ok(None);
    }
    reader
        .read_exact(&mut h[1..])
        .map_err(|_| bad("truncated block header"))?;
    // The audio payload ends before optional APEv2 / ID3v1 tags.
    if &h[..8] == b"APETAGEX" || &h[..3] == b"TAG" {
        return Ok(None);
    }
    if &h[..4] != b"wvpk" {
        return Err(bad("invalid block signature"));
    }
    let len = u64::from(u32le(&h, 4)) + 8;
    if len < 32 || len > MAX_BLOCK_BYTES as u64 || len % 2 != 0 {
        return Err(bad("block byte budget/size exceeded"));
    }
    let mut b = Vec::new();
    b.try_reserve_exact(len as usize)
        .map_err(|_| CodecError::OutOfMemory)?;
    b.extend_from_slice(&h);
    b.resize(len as usize, 0);
    reader
        .read_exact(&mut b[32..])
        .map_err(|_| bad("truncated block payload"))?;
    Ok(Some(b))
}
pub(crate) fn probe(reader: &mut impl Read) -> Result<AudioInfo, CodecError> {
    let block = read_block(reader)?.ok_or(CodecError::NoAudio)?;
    Ok(header(&block)?.0)
}
pub(crate) fn decode(
    reader: &mut impl Read,
    options: &DecodeOptions,
) -> Result<AudioBuffer, CodecError> {
    let mut info: Option<AudioInfo> = None;
    let mut pcm = Vec::new();
    let mut frames = 0_u64;
    let mut blocks = 0_u32;
    while let Some(block) = read_block(reader)? {
        blocks += 1;
        if blocks > 1_000_001 {
            return Err(bad("block count budget exceeded"));
        }
        // Reference encoders may write a metadata-only trailing wrapper/MD5
        // block whose audio layout flags are zero. They do not change the
        // stream's format and still receive metadata/checksum validation.
        if u32le(&block, 20) == 0 && info.is_some() {
            let version = u16::from_le_bytes([block[8], block[9]]);
            if !(0x402..=0x410).contains(&version) {
                return Err(unsupported("only version 4/5 blocks are supported"));
            }
            decode_block(&block, 1, u32le(&block, 24))?;
            continue;
        }
        let (current, coded, flags) = header(&block)?;
        if let Some(first) = &info
            && (current.sample_rate != first.sample_rate
                || current.channels != first.channels
                || current.bits_per_sample != first.bits_per_sample)
        {
            return Err(bad("audio format changes between blocks"));
        }
        let count = u64::from(u32le(&block, 20));
        if count > 0 && u64::from(u32le(&block, 16)) != frames {
            return Err(bad("noncontiguous block index"));
        }
        let next = frames + count;
        if next * u64::from(current.channels) * 4 > options.max_decoded_bytes {
            return Err(CodecError::TooLarge {
                limit_bytes: options.max_decoded_bytes,
            });
        }
        let decoded = decode_block(&block, coded, flags)?;
        pcm.try_reserve(decoded.len() * usize::from(current.channels) / coded)
            .map_err(|_| CodecError::OutOfMemory)?;
        let scale = (1_u32 << (current.bits_per_sample.unwrap_or(16) - 1)) as f32;
        for pair in decoded.chunks_exact(coded) {
            pcm.push(pair[0] as f32 / scale);
            if current.channels == 2 {
                pcm.push(pair[if coded == 2 { 1 } else { 0 }] as f32 / scale);
            }
        }
        frames = next;
        if info.is_none() {
            info = Some(current);
        }
    }
    let info = info.ok_or(CodecError::NoAudio)?;
    if info.frames.is_some_and(|n| n != frames) {
        return Err(bad("total frame count mismatch"));
    }
    Ok(AudioBuffer::from_interleaved(
        info.sample_rate,
        info.channels,
        pcm,
    ))
}
fn subblock(out: &mut Vec<u8>, id: u8, bytes: &[u8]) {
    let words = bytes.len().div_ceil(2);
    let id = id
        | if !bytes.len().is_multiple_of(2) {
            0x40
        } else {
            0
        };
    out.push(id | if words > 255 { 0x80 } else { 0 });
    out.push(words as u8);
    if words > 255 {
        out.push((words >> 8) as u8);
        out.push((words >> 16) as u8);
    }
    out.extend_from_slice(bytes);
    if !bytes.len().is_multiple_of(2) {
        out.push(0);
    }
}

/// A bounded streaming writer. PCM is quantized once to the chosen integer
/// depth without dither; already quantized PCM is preserved bit for bit.
#[derive(Debug)]
pub struct WavPackWriter {
    file: AtomicFile,
    rate: u32,
    channels: u16,
    depth: WavPackDepth,
    block_frames: usize,
    pending: Vec<i32>,
    frames: u32,
    offsets: Vec<u64>,
    bytes: u64,
    broken: bool,
    tags: Vec<u8>,
}
impl WavPackWriter {
    pub fn create(
        path: impl AsRef<Path>,
        rate: u32,
        channels: u16,
        depth: WavPackDepth,
    ) -> Result<Self, CodecError> {
        Self::create_with_tags(
            path,
            rate,
            channels,
            depth,
            DEFAULT_WAVPACK_BLOCK_FRAMES,
            &AudioTags::default(),
        )
    }
    pub fn create_with_tags(
        path: impl AsRef<Path>,
        rate: u32,
        channels: u16,
        depth: WavPackDepth,
        block_frames: usize,
        tags: &AudioTags<'_>,
    ) -> Result<Self, CodecError> {
        check_layout(rate, channels)?;
        if !(1..=65536).contains(&block_frames) {
            return Err(CodecError::InvalidInput(
                "WavPack block size must be 1..65536 frames".into(),
            ));
        }
        let tags = ape_tags(tags)?;
        Ok(Self {
            file: AtomicFile::create(path.as_ref())?,
            rate,
            channels,
            depth,
            block_frames,
            pending: Vec::with_capacity(block_frames * usize::from(channels)),
            frames: 0,
            offsets: Vec::new(),
            bytes: 0,
            broken: false,
            tags,
        })
    }
    pub fn write(&mut self, pcm: &[f32]) -> Result<(), CodecError> {
        if self.broken {
            return Err(CodecError::Encoding("WavPack writer failed earlier".into()));
        }
        if !pcm.len().is_multiple_of(usize::from(self.channels))
            || pcm.iter().any(|s| !s.is_finite())
        {
            self.broken = true;
            return Err(CodecError::InvalidInput(
                "WavPack requires finite, complete PCM frames".into(),
            ));
        }
        self.broken = true;
        let scale = (1_u32 << (self.depth.bits() - 1)) as f64;
        for &s in pcm {
            self.pending.push(
                (f64::from(s).clamp(-1.0, 1.0) * scale)
                    .round()
                    .clamp(-scale, scale - 1.0) as i32,
            );
            if self.pending.len() == self.block_frames * usize::from(self.channels) {
                self.flush_block()?;
            }
        }
        self.broken = false;
        Ok(())
    }
    fn flush_block(&mut self) -> Result<(), CodecError> {
        if self.offsets.len() >= 1_000_000 {
            return Err(CodecError::Encoding(
                "WavPack block count budget exceeded".into(),
            ));
        }
        let count = (self.pending.len() / usize::from(self.channels)) as u32;
        let total = self
            .frames
            .checked_add(count)
            .filter(|v| *v != u32::MAX)
            .ok_or_else(|| CodecError::Encoding("WavPack sample count exceeded".into()))?;
        let bits = self.depth.bits();
        let rate_index = RATES.iter().position(|r| *r == self.rate).unwrap_or(15) as u32;
        let flags = (bits / 8 - 1)
            | if self.channels == 1 { 4 } else { 0 }
            | 0x1800
            | ((bits - 1) << 18)
            | (rate_index << 23);
        let crc = self.pending.iter().fold(u32::MAX, |crc, &v| {
            crc.wrapping_mul(3).wrapping_add(v as u32)
        });
        let mut block = b"wvpk".to_vec();
        block.extend_from_slice(&[0; 4]);
        block.extend_from_slice(&[0x10, 4, 0, 0]);
        for n in [u32::MAX, self.frames, count, flags, crc] {
            block.extend_from_slice(&n.to_le_bytes());
        }
        if self.offsets.is_empty() {
            subblock(
                &mut block,
                0x21,
                &riff_header(self.rate, self.channels, bits, 0),
            );
        }
        if rate_index == 15 {
            subblock(&mut block, 0x27, &self.rate.to_le_bytes()[..3]);
        }
        subblock(&mut block, 2, &[0x46]); // term 1, delta 2
        subblock(&mut block, 3, &vec![0; usize::from(self.channels)]);
        subblock(&mut block, 4, &vec![0; usize::from(self.channels) * 2]);
        let log = ((bits + 1) * 256) as i16;
        let initial = exp(log)? as u32;
        let mut vars = Vec::new();
        for _ in 0..usize::from(self.channels) * 3 {
            vars.extend_from_slice(&log.to_le_bytes());
        }
        subblock(&mut block, 5, &vars);
        let mut pass = Pass {
            term: 1,
            delta: 2,
            ..Default::default()
        };
        for pair in self.pending.chunks_exact_mut(usize::from(self.channels)) {
            pass.positive(pair, true);
        }
        let mut med = [[initial; 3]; 2];
        if self.channels == 1 {
            med[1] = [0; 3];
        }
        subblock(
            &mut block,
            10,
            &entropy_encode(&self.pending, usize::from(self.channels), med)?,
        );
        let size = block.len() as u32 - 8;
        block[4..8].copy_from_slice(&size.to_le_bytes());
        self.file.write_all(&block)?;
        self.offsets.push(self.bytes);
        self.bytes += block.len() as u64;
        self.frames = total;
        self.pending.clear();
        Ok(())
    }
    pub fn finalize(mut self) -> Result<(), CodecError> {
        if self.broken {
            return Err(CodecError::Encoding("WavPack writer failed earlier".into()));
        }
        if !self.pending.is_empty() || self.offsets.is_empty() {
            self.flush_block()?;
        }
        // PCM data's RIFF size must fit the wrapper even when compression is tiny.
        let riff = riff_header(self.rate, self.channels, self.depth.bits(), self.frames);
        let bytes =
            u64::from(self.frames) * u64::from(self.channels) * u64::from(self.depth.bits() / 8);
        if bytes + bytes % 2 > u64::from(u32::MAX) - 36 {
            return Err(CodecError::WavTooLarge);
        }
        if bytes % 2 != 0 {
            let mut trailer = b"wvpk".to_vec();
            trailer.extend_from_slice(&[0; 4]);
            trailer.extend_from_slice(&[0x10, 4, 0, 0]);
            let flags = (self.depth.bits() / 8 - 1)
                | if self.channels == 1 { 4 } else { 0 }
                | 0x1800
                | ((self.depth.bits() - 1) << 18)
                | (15 << 23);
            for n in [self.frames, self.frames, 0, flags, u32::MAX] {
                trailer.extend_from_slice(&n.to_le_bytes());
            }
            subblock(&mut trailer, 0x27, &self.rate.to_le_bytes()[..3]);
            subblock(&mut trailer, 0x22, &[0]);
            let size = trailer.len() as u32 - 8;
            trailer[4..8].copy_from_slice(&size.to_le_bytes());
            self.file.write_all(&trailer)?;
        }
        self.file.write_all(&self.tags)?;
        let total = self.frames;
        let offsets = self.offsets;
        self.file.place(|file| {
            for at in offsets {
                file.seek(SeekFrom::Start(at + 12))?;
                file.write_all(&total.to_le_bytes())?;
            }
            // The first sub-block is a 44-byte RIFF header: its payload starts at 34.
            file.seek(SeekFrom::Start(34))?;
            file.write_all(&riff)
        })?;
        Ok(())
    }
}
fn riff_header(rate: u32, channels: u16, bits: u32, frames: u32) -> Vec<u8> {
    let size = frames
        .saturating_mul(u32::from(channels))
        .saturating_mul(bits / 8);
    let mut b = b"RIFF".to_vec();
    b.extend_from_slice(&size.saturating_add(36 + size % 2).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16_u32.to_le_bytes());
    b.extend_from_slice(&1_u16.to_le_bytes());
    b.extend_from_slice(&channels.to_le_bytes());
    b.extend_from_slice(&rate.to_le_bytes());
    b.extend_from_slice(&(rate * u32::from(channels) * (bits / 8)).to_le_bytes());
    b.extend_from_slice(&(channels * (bits / 8) as u16).to_le_bytes());
    b.extend_from_slice(&(bits as u16).to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&size.to_le_bytes());
    b
}
fn ape_tags(tags: &AudioTags<'_>) -> Result<Vec<u8>, CodecError> {
    let mut body = Vec::new();
    let mut count = 0_u32;
    for (key, value) in [
        ("Title", tags.title),
        ("Artist", tags.author),
        ("Genre", tags.genre),
        ("Comment", tags.comments),
    ] {
        if value.len() > 16384 || value.contains('\0') {
            return Err(CodecError::InvalidInput(
                "WavPack tag exceeds 16384 bytes or contains NUL".into(),
            ));
        }
        if value.is_empty() {
            continue;
        }
        count += 1;
        body.extend_from_slice(&(value.len() as u32).to_le_bytes());
        body.extend_from_slice(&0_u32.to_le_bytes());
        body.extend_from_slice(key.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
    }
    if count != 0 {
        let size = body.len() as u32 + 32;
        let mut header = b"APETAGEX".to_vec();
        for n in [2000_u32, size, count, 0xa000_0000, 0, 0] {
            header.extend_from_slice(&n.to_le_bytes());
        }
        body.extend_from_slice(b"APETAGEX");
        for n in [2000, size, count, 0, 0, 0] {
            body.extend_from_slice(&n.to_le_bytes());
        }
        header.extend(body);
        return Ok(header);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_predictor_term_has_measured_history_and_cross_channel_results() {
        for term in 1..=8 {
            let mut pass = Pass {
                term,
                delta: 0,
                weights: [1024; 2],
                ..Default::default()
            };
            let mut output = Vec::new();
            for _ in 0..24 {
                let mut pair = [1, -2];
                pass.decode(&mut pair);
                output.push(pair);
            }
            for (i, pair) in output.into_iter().enumerate() {
                let multiples = (i / term as usize + 1) as i32;
                assert_eq!(pair, [multiples, -2 * multiples], "term {term} frame {i}");
            }
        }
        for (term, expected) in [
            (17, vec![[1, -2], [3, -6], [6, -12]]),
            (18, vec![[1, -2], [2, -5], [3, -9]]),
            (-1, vec![[1, -1], [0, -2], [-1, -3]]),
            (-2, vec![[-1, -2], [-2, -3], [-3, -4]]),
            (-3, vec![[1, -2], [-1, -1], [0, -3]]),
        ] {
            let mut pass = Pass {
                term,
                delta: 0,
                weights: [1024; 2],
                ..Default::default()
            };
            for pair in expected {
                let mut residual = [1, -2];
                pass.decode(&mut residual);
                assert_eq!(residual, pair, "term {term}");
            }
        }
        let mut weight = 1024;
        update(&mut weight, 1, 1, 7, true);
        assert_eq!(weight, 1024);
        update(&mut weight, 1, -1, 7, true);
        assert_eq!(weight, 1017);
        update(&mut weight, 0, 1, 7, true);
        assert_eq!(weight, 1017);
    }

    fn fixture(
        residuals: &[i32],
        channels: usize,
        flags: u32,
        decorrelation: (&[u8], &[u8], &[u8]),
        integer: Option<[u8; 4]>,
        expected_before_fix: &[i32],
    ) -> Vec<u8> {
        let (terms, weights, history) = decorrelation;
        let frames = (residuals.len() / channels) as u32;
        let crc = expected_before_fix.iter().fold(u32::MAX, |crc, &s| {
            crc.wrapping_mul(3).wrapping_add(s as u32)
        });
        let mut b = b"wvpk".to_vec();
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&[0x10, 4, 0, 0]);
        for v in [frames, 0, frames, flags | 0x1800 | (10 << 23), crc] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        subblock(&mut b, 2, terms);
        subblock(&mut b, 3, weights);
        subblock(&mut b, 4, history);
        subblock(&mut b, 5, &vec![0; channels * 6]);
        if let Some(integer) = integer {
            subblock(&mut b, 9, &integer);
        }
        subblock(
            &mut b,
            10,
            &entropy_encode(residuals, channels, [[0; 3]; 2]).unwrap(),
        );
        let len = b.len() as u32 - 8;
        b[4..8].copy_from_slice(&len.to_le_bytes());
        b
    }
    #[test]
    fn cascaded_pass_order_joint_and_false_stereo_and_integer_restoration_are_exact() {
        let mut h = Vec::new(); // 1st encode pass term17: previous samples 1,0.
        h.extend_from_slice(&256_i16.to_le_bytes());
        h.extend_from_slice(&0_i16.to_le_bytes());
        // 2nd encode pass term1: previous sample zero.
        h.extend_from_slice(&0_i16.to_le_bytes());
        let b = fixture(
            &[1, -2],
            1,
            1 | 4 | (15 << 18),
            (&[22, 6], &[127, 127], &h),
            None,
            &[3, 4],
        );
        assert_eq!(
            decode(&mut &b[..], &Default::default()).unwrap().samples(),
            &[3.0 / 32768.0, 4.0 / 32768.0]
        );
        // joint stereo: side=1, mid=-2 => right=-2, left=-1.
        let b = fixture(
            &[1, -2],
            2,
            1 | 16 | (15 << 18),
            (&[], &[], &[]),
            None,
            &[-1, -2],
        );
        assert_eq!(
            decode(&mut &b[..], &Default::default()).unwrap().samples(),
            &[-1.0 / 32768.0, -2.0 / 32768.0]
        );
        let b = fixture(
            &[1, -2],
            1,
            1 | 0x4000_0000 | (15 << 18),
            (&[], &[], &[]),
            None,
            &[1, -2],
        );
        assert_eq!(
            decode(&mut &b[..], &Default::default()).unwrap().samples(),
            &[1.0 / 32768.0, 1.0 / 32768.0, -2.0 / 32768.0, -2.0 / 32768.0]
        );
        for (integer, expected) in [
            ([0, 2, 0, 0], [4, -8]),
            ([0, 0, 2, 0], [7, -5]),
            ([0, 0, 0, 2], [7, -8]),
        ] {
            let b = fixture(
                &[1, -2],
                1,
                1 | 4 | 0x100 | (15 << 18),
                (&[], &[], &[]),
                Some(integer),
                &[1, -2],
            );
            assert_eq!(
                decode(&mut &b[..], &Default::default()).unwrap().samples(),
                &expected.map(|v| v as f32 / 32768.0)
            );
        }
    }
}
