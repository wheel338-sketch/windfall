use crate::{AnalysisError, CHUNK_BYTES, CHUNK_SAMPLES, Result, Work, add};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use windfall_core::{AudioBuffer, AudioIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameRange {
    pub start: u64,
    pub end: u64,
}
impl FrameRange {
    pub fn frames(self) -> Result<u64> {
        self.end
            .checked_sub(self.start)
            .filter(|n| *n > 0)
            .ok_or(AnalysisError::Invalid("empty/reversed selection"))
    }
    pub fn validate(self, frames: u64) -> Result<()> {
        self.frames()?;
        if self.end > frames {
            return Err(AnalysisError::Invalid("selection outside input"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioShape {
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
}
impl AudioShape {
    pub fn of(audio: &AudioBuffer) -> Self {
        Self {
            frames: audio.frames() as u64,
            channels: audio.channels(),
            sample_rate: audio.sample_rate(),
        }
    }
    pub fn pcm_bytes(self) -> Result<u64> {
        self.frames
            .checked_mul(u64::from(self.channels))
            .and_then(|n| n.checked_mul(4))
            .ok_or(AnalysisError::Exhausted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FingerprintKind {
    FileBytes,
    PcmF32LeV1,
}

/// Content identity, never a pathname, mtime or size-only cache key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentFingerprint {
    pub kind: FingerprintKind,
    pub sha256: [u8; 32],
    pub bytes: u64,
}
impl ContentFingerprint {
    pub fn reader(mut reader: impl Read, maximum: u64, work: &mut Work) -> Result<Self> {
        let mut bytes = 0;
        let mut sha = Sha256::new();
        let mut chunk = [0; CHUNK_BYTES];
        loop {
            work.check()?;
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            bytes = add(bytes, n as u64)?;
            if bytes > maximum {
                return Err(AnalysisError::Budget("fingerprint bytes"));
            }
            work.checkpoint(n as u64)?;
            sha.update(&chunk[..n]);
        }
        work.check()?;
        Ok(Self {
            kind: FingerprintKind::FileBytes,
            sha256: sha.finalize().into(),
            bytes,
        })
    }
    pub fn audio(audio: &AudioBuffer, work: &mut Work) -> Result<Self> {
        let mut sha = Sha256::new();
        sha.update(b"windfall-pcm-f32le-v1\0");
        sha.update(audio.sample_rate().to_le_bytes());
        sha.update(audio.channels().to_le_bytes());
        sha.update((audio.frames() as u64).to_le_bytes());
        for chunk in audio.samples().chunks(CHUNK_SAMPLES) {
            work.checkpoint(chunk.len() as u64)?;
            for sample in chunk {
                if !sample.is_finite() {
                    return Err(AnalysisError::Invalid("nonfinite input"));
                }
                sha.update(sample.to_bits().to_le_bytes());
            }
        }
        Ok(Self {
            kind: FingerprintKind::PcmF32LeV1,
            sha256: sha.finalize().into(),
            bytes: AudioShape::of(audio).pcm_bytes()?,
        })
    }
}

/// The Session supplies these canonical fields from one recording-excluded
/// snapshot. `binding` hashes the exact logical source/clip transform contract,
/// not an OS path spelling. `source_identity` comes from the actually loaded
/// source; a prepared editor view may have a different allocation. File-byte
/// fingerprints must be captured from the exact bytes decoded, and rechecked
/// off State before the final atomic Session guards. No constructor can prove
/// that an externally supplied digest was verified correctly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureStamp {
    pub generation: u64,
    pub edit_revision: u64,
    pub source_key: u64,
    pub binding: [u8; 32],
    pub source_identity: AudioIdentity,
    pub source_fingerprint: ContentFingerprint,
    pub input_shape: AudioShape,
    pub selection: FrameRange,
}

#[derive(Debug, Clone, Copy)]
pub struct InputLimits {
    pub max_frames: u64,
    pub max_bytes: u64,
    pub max_channels: u16,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
}
impl Default for InputLimits {
    fn default() -> Self {
        Self {
            max_frames: 192_000 * 120,
            max_bytes: 64 * 1024 * 1024,
            max_channels: 2,
            min_sample_rate: 8_000,
            max_sample_rate: 192_000,
        }
    }
}

/// A compact owned copy prevents a tiny view from retaining an unobservable
/// oversized Arc<Vec> capacity. Capture/copy/finite scan runs off all State and
/// audio locks. The original source AudioIdentity remains part of the stamp.
#[derive(Debug)]
pub struct CapturedInput {
    audio: AudioBuffer,
    pcm_file: Option<tempfile::NamedTempFile>,
    stamp: CaptureStamp,
    retained_bytes: u64,
}
impl CapturedInput {
    pub fn capture(
        audio: &AudioBuffer,
        stamp: CaptureStamp,
        limits: InputLimits,
        work: &mut Work,
    ) -> Result<Self> {
        let shape = AudioShape::of(audio);
        if !stamp.source_identity.is_live() || stamp.source_fingerprint.bytes == 0 {
            return Err(AnalysisError::Invalid("unverified/expired source capture"));
        }
        if shape != stamp.input_shape {
            return Err(AnalysisError::Invalid("capture shape mismatch"));
        }
        stamp.selection.validate(shape.frames)?;
        if shape.frames == 0
            || shape.frames > limits.max_frames
            || shape.channels > limits.max_channels
            || shape.sample_rate < limits.min_sample_rate
            || shape.sample_rate > limits.max_sample_rate
            || limits.min_sample_rate == 0
            || limits.max_channels == 0
        {
            return Err(AnalysisError::Invalid("input shape bounds"));
        }
        let bytes = shape.pcm_bytes()?;
        if bytes > limits.max_bytes {
            return Err(AnalysisError::Budget("input bytes"));
        }
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(audio.samples().len())
            .map_err(|_| AnalysisError::Budget("input allocation"))?;
        let retained_bytes = (samples.capacity() as u64)
            .checked_mul(4)
            .ok_or(AnalysisError::Exhausted)?;
        if retained_bytes > limits.max_bytes {
            return Err(AnalysisError::Budget("input capacity"));
        }
        for chunk in audio.samples().chunks(CHUNK_SAMPLES) {
            work.checkpoint(chunk.len() as u64)?;
            if chunk.iter().any(|x| !x.is_finite()) {
                return Err(AnalysisError::Invalid("nonfinite input"));
            }
            samples.extend_from_slice(chunk);
        }
        work.check()?;
        Ok(Self {
            audio: AudioBuffer::from_interleaved(shape.sample_rate, shape.channels, samples),
            pcm_file: None,
            stamp,
            retained_bytes,
        })
    }
    pub fn stamp(&self) -> &CaptureStamp {
        &self.stamp
    }
    /// File-backed rendered capture; caller emits sequential, whole-frame,
    /// finite chunks. The spool is owned until job cancellation/consumption.
    pub fn capture_stream(root: &std::path::Path, stamp: CaptureStamp, limits: InputLimits,
        work: &mut Work, render: &mut dyn FnMut(u64, &mut [f32], &mut Work) -> Result<()>) -> Result<Self> {
        use std::io::Write;
        let shape = stamp.input_shape;
        stamp.selection.validate(shape.frames)?;
        shape.pcm_bytes()?;
        if !stamp.source_identity.is_live() || stamp.source_fingerprint.bytes == 0
            || shape.frames > limits.max_frames || !(1..=limits.max_channels).contains(&shape.channels)
            || !(1..=2).contains(&shape.channels) || shape.sample_rate == 0 || limits.max_bytes < 16*1024
            || !(limits.min_sample_rate..=limits.max_sample_rate).contains(&shape.sample_rate) {
            return Err(AnalysisError::Invalid("stream capture shape/source"));
        }
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        let channels = shape.channels as usize;
        let mut samples = vec![0.0; 4096/channels*channels];
        let mut start = 0;
        while start < shape.frames {
            let frames = (shape.frames-start).min((samples.len()/channels) as u64) as usize;
            let block = &mut samples[..frames*channels];
            work.check()?;
            render(start, block, work)?;
            let mut bytes = [0u8; 16384];
            for (x, b) in block.iter().zip(bytes.chunks_exact_mut(4)) {
                if !x.is_finite() { return Err(AnalysisError::Invalid("nonfinite streamed capture")); }
                b.copy_from_slice(&x.to_le_bytes());
            }
            file.write_all(&bytes[..block.len()*4])?;
            work.checkpoint(block.len() as u64)?;
            start += frames as u64;
        }
        file.as_file().sync_all()?;
        work.check()?;
        Ok(Self { audio: AudioBuffer::from_interleaved(shape.sample_rate, shape.channels, vec![]),
            pcm_file: Some(file), stamp, retained_bytes: 16*1024 })
    }
    pub fn disk_bytes(&self) -> Result<u64> {
        if self.pcm_file.is_some() { self.stamp.input_shape.pcm_bytes() } else { Ok(0) }
    }
    pub fn frame_source(&self) -> Result<Box<dyn crate::separation::FrameSource + '_>> {
        let mut shape = self.stamp.input_shape;
        shape.frames = self.stamp.selection.frames()?;
        if let Some(file) = &self.pcm_file {
            Ok(Box::new(crate::separation::PcmSource { file: file.reopen()?, shape, origin: self.stamp.selection.start }))
        } else {
            Ok(Box::new(crate::separation::SliceSource { samples: self.selected_samples(), shape }))
        }
    }
    pub fn audio(&self) -> &AudioBuffer {
        assert!(self.pcm_file.is_none(), "file-backed captures require frame_source()");
        &self.audio
    }
    pub fn selected_samples(&self) -> &[f32] {
        assert!(self.pcm_file.is_none(), "file-backed captures require frame_source()");
        let channels = usize::from(self.audio.channels());
        &self.audio.samples()[self.stamp.selection.start as usize * channels
            ..self.stamp.selection.end as usize * channels]
    }
    pub fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }
}
