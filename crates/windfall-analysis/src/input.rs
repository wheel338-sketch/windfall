use crate::{AnalysisError, CHUNK_BYTES, CHUNK_SAMPLES, Result, Work, add};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
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
    audio: Option<AudioBuffer>,
    file: Option<std::sync::Mutex<File>>,
    stamp: CaptureStamp,
    retained_bytes: u64,
}
type CaptureRenderer<'a> = dyn FnMut(u64, &mut [f32], &mut Work) -> Result<()> + 'a;
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
            audio: Some(AudioBuffer::from_interleaved(
                shape.sample_rate,
                shape.channels,
                samples,
            )),
            file: None,
            stamp,
            retained_bytes,
        })
    }
    pub fn stamp(&self) -> &CaptureStamp {
        &self.stamp
    }
    /// Adapt a bounded random-access renderer to selected-only anonymous storage.
    pub fn capture_frames(
        root: &Path,
        stamp: CaptureStamp,
        limits: InputLimits,
        work: &mut Work,
        render: &mut CaptureRenderer<'_>,
    ) -> Result<Self> {
        let shape = stamp.input_shape;
        Self::capture_stream(root, stamp, limits, work, |work, emit| {
            let channels = usize::from(shape.channels);
            if channels == 0 || channels > 2 {
                return Err(AnalysisError::Invalid("capture channels"));
            }
            let mut samples = [0.0; CHUNK_SAMPLES];
            let mut start = 0;
            while start < shape.frames {
                let frames = (shape.frames - start).min((CHUNK_SAMPLES / channels) as u64) as usize;
                let block = &mut samples[..frames * channels];
                work.check()?;
                render(start, block, work)?;
                emit(block, work)?;
                start += frames as u64;
            }
            work.check()
        })
    }
    pub fn frame_source(&self) -> Result<Box<dyn crate::separation::FrameSource + '_>> {
        let mut shape = self.stamp.input_shape;
        shape.frames = self.stamp.selection.frames()?;
        if let Some(file) = &self.file {
            let guard = crate::lock(file);
            // Cloned handles share offsets; hold the original reader lock until
            // this source is retired so every selected reader has its own clock.
            let source = crate::separation::PcmSource {
                file: guard.try_clone()?,
                shape,
                origin: 0,
            };
            Ok(Box::new(CapturedFileSource {
                source,
                _guard: guard,
            }))
        } else {
            Ok(Box::new(crate::separation::SliceSource {
                samples: self.selected_samples(),
                shape,
            }))
        }
    }
    pub fn audio(&self) -> &AudioBuffer {
        self.audio
            .as_ref()
            .expect("memory input required by this adapter")
    }
    pub fn selected_samples(&self) -> &[f32] {
        let audio = self.audio();
        let channels = usize::from(audio.channels());
        &audio.samples()[self.stamp.selection.start as usize * channels
            ..self.stamp.selection.end as usize * channels]
    }
    pub fn retained_bytes(&self) -> u64 {
        self.retained_bytes
    }
    pub fn is_file_backed(&self) -> bool {
        self.file.is_some()
    }
    pub fn disk_bytes(&self) -> Result<u64> {
        if self.is_file_backed() {
            self.stamp
                .selection
                .frames()?
                .checked_mul(u64::from(self.stamp.input_shape.channels) * 4)
                .ok_or(AnalysisError::Exhausted)
        } else {
            Ok(0)
        }
    }
    /// Capture the selected immutable Float32 view into an anonymous temporary
    /// file. It has no mutable pathname and closes/deletes on every error,
    /// cancellation and retirement. The producer never emits more than 4096
    /// samples. The complete producer view is checked, but only selection bytes
    /// occupy disk. Shape/disk admission precedes all writes. Native dereverb only.
    pub fn capture_stream(
        root: &Path,
        stamp: CaptureStamp,
        limits: InputLimits,
        work: &mut Work,
        produce: impl FnOnce(&mut Work, &mut dyn FnMut(&[f32], &mut Work) -> Result<()>) -> Result<()>,
    ) -> Result<Self> {
        let shape = stamp.input_shape;
        stamp.selection.validate(shape.frames)?;
        if !stamp.source_identity.is_live()
            || stamp.source_fingerprint.bytes == 0
            || limits.min_sample_rate == 0
            || !matches!(shape.channels, 1 | 2)
            || !(1..=limits.max_channels).contains(&shape.channels)
            || !(limits.min_sample_rate..=limits.max_sample_rate).contains(&shape.sample_rate)
            || shape.frames > limits.max_frames
        {
            return Err(AnalysisError::Invalid("stream capture shape/identity"));
        }
        let bytes = shape.pcm_bytes()?;
        let selected_first = stamp
            .selection
            .start
            .checked_mul(u64::from(shape.channels) * 4)
            .ok_or(AnalysisError::Exhausted)?;
        let selected_last = stamp
            .selection
            .end
            .checked_mul(u64::from(shape.channels) * 4)
            .ok_or(AnalysisError::Exhausted)?;
        if selected_last - selected_first > limits.max_bytes {
            return Err(AnalysisError::Budget("stream capture disk bytes"));
        }
        work.check()?;
        let mut file = tempfile::tempfile_in(root)?;
        let mut written = 0_u64;
        produce(work, &mut |samples, work| {
            work.checkpoint(samples.len() as u64)?;
            if samples.len() > CHUNK_SAMPLES
                || !samples.len().is_multiple_of(usize::from(shape.channels))
                || samples.iter().any(|s| !s.is_finite())
            {
                return Err(AnalysisError::Invalid("stream capture block"));
            }
            let first_byte = written;
            written = add(written, samples.len() as u64 * 4)?;
            if written > bytes {
                return Err(AnalysisError::Budget("stream capture shape bytes"));
            }
            let mut encoded = [0_u8; CHUNK_BYTES];
            for (target, sample) in encoded.as_chunks_mut::<4>().0.iter_mut().zip(samples) {
                target.copy_from_slice(&sample.to_le_bytes());
            }
            let from = first_byte.max(selected_first);
            let to = written.min(selected_last);
            if from < to {
                file.write_all(&encoded[(from - first_byte) as usize..(to - first_byte) as usize])?;
            }
            Ok(())
        })?;
        work.check()?;
        if written != bytes {
            return Err(AnalysisError::Invalid("incomplete stream capture"));
        }
        file.flush()?;
        file.seek(SeekFrom::Start(0))?;
        Ok(Self {
            audio: None,
            file: Some(std::sync::Mutex::new(file)),
            stamp,
            // No PCM, pathname, or read buffers survive capture: only the
            // inline stamp, weak identity, mutex and OS handle remain. Reserve
            // a full chunk for this compact metadata; working read buffers
            // belong to the consuming algorithm's scratch allowance.
            retained_bytes: CHUNK_BYTES as u64,
        })
    }
    /// Selected PCM in bounded whole-frame blocks, from either storage kind.
    pub fn for_each_selected(
        &self,
        work: &mut Work,
        mut emit: impl FnMut(&[f32], &mut Work) -> Result<()>,
    ) -> Result<()> {
        if let Some(audio) = &self.audio {
            let channels = usize::from(audio.channels());
            let selected = &audio.samples()[self.stamp.selection.start as usize * channels
                ..self.stamp.selection.end as usize * channels];
            for block in selected.chunks(CHUNK_SAMPLES) {
                work.check()?;
                emit(block, work)?;
            }
            return work.check();
        }
        // A cloned OS handle shares its seek position. Serialize control-side
        // readers so independent auditions cannot change one another's origin.
        let file = self
            .file
            .as_ref()
            .ok_or(AnalysisError::Invalid("missing captured input"))?;
        let mut file = crate::lock(file);
        let channels = u64::from(self.stamp.input_shape.channels);
        file.seek(SeekFrom::Start(0))?;
        let mut remaining = self.stamp.selection.frames()? * channels;
        let mut encoded = [0_u8; CHUNK_BYTES];
        let mut samples = [0_f32; CHUNK_SAMPLES];
        while remaining > 0 {
            work.check()?;
            let count = remaining.min(CHUNK_SAMPLES as u64) as usize;
            file.read_exact(&mut encoded[..count * 4])?;
            for (sample, bytes) in samples[..count]
                .iter_mut()
                .zip(encoded.as_chunks::<4>().0.iter())
            {
                *sample = f32::from_le_bytes(*bytes);
            }
            work.checkpoint(count as u64)?;
            emit(&samples[..count], work)?;
            remaining -= count as u64;
        }
        work.check()
    }
}

struct CapturedFileSource<'a> {
    source: crate::separation::PcmSource,
    _guard: std::sync::MutexGuard<'a, File>,
}
impl crate::separation::FrameSource for CapturedFileSource<'_> {
    fn shape(&self) -> AudioShape {
        self.source.shape
    }
    fn read_frames(&mut self, start: u64, output: &mut [f32], work: &mut Work) -> Result<()> {
        crate::separation::FrameSource::read_frames(&mut self.source, start, output, work)
    }
}
