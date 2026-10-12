use crate::{
    AnalysisError, AudioShape, CHUNK_BYTES, CHUNK_SAMPLES, ContentFingerprint, ModelManifest,
    Result, Work, add,
};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempDir};
use windfall_codec::{DecodeOptions, WavSampleFormat, WavWriter};

/// Flat portable names only. No directories, drive prefixes, ADS, traversal,
/// trailing dots/spaces or Windows device names enter an output path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelativeName(String);
impl RelativeName {
    pub fn new(name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if name.is_empty()
            || name.len() > 96
            || name.starts_with('.')
            || name.ends_with('.')
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            || name.contains("..")
            || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.as_bytes()[3].is_ascii_digit())
        {
            return Err(AnalysisError::Invalid("artifact relative name"));
        }
        // Keep metadata bounded even when a native adapter supplied a String
        // with small length and arbitrarily large unused capacity.
        Ok(Self(name.into_boxed_str().into_string()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub name: String,
    pub role: String,
    pub shape: AudioShape,
    pub frame_origin: u64,
    pub bytes: u64,
    pub sha256: [u8; 32],
    /// Set only after a successful no-clobber publication; never GC'd here.
    pub published_path: Option<PathBuf>,
}
pub(crate) struct Artifact {
    pub metadata: ArtifactMetadata,
    pub staged: PathBuf,
}
struct ActiveOutput {
    name: RelativeName,
    role: String,
    shape: AudioShape,
    frames: u64,
    writer: WavWriter,
    estimate: u64,
}

/// Streaming adapter output, backed solely by this job's private directory.
/// Exactly one WAV writer is active; completed audio is not retained in RAM.
pub struct ArtifactWriter {
    // Drop open codec files before the directory on every error/panic path.
    active: Option<ActiveOutput>,
    manifest: ModelManifest,
    frames: u64,
    origin: u64,
    maximum_disk: u64,
    used_disk: u64,
    artifacts: Vec<Artifact>,
    directory: TempDir,
}
impl ArtifactWriter {
    pub(crate) fn path(&self) -> &Path {
        self.directory.path()
    }
    pub(crate) fn new(
        root: &Path,
        manifest: &ModelManifest,
        frames: u64,
        origin: u64,
        maximum_disk: u64,
    ) -> Result<Self> {
        Ok(Self {
            directory: tempfile::Builder::new().prefix("job-").tempdir_in(root)?,
            manifest: manifest.clone(),
            frames,
            origin,
            maximum_disk,
            used_disk: 0,
            active: None,
            artifacts: Vec::new(),
        })
    }
    fn shape_for(&self, role: &str) -> Result<AudioShape> {
        let role = self
            .manifest
            .outputs
            .iter()
            .find(|r| r.role == role)
            .ok_or(AnalysisError::Invalid("unknown artifact role"))?;
        Ok(AudioShape {
            frames: self.frames,
            channels: role.channels,
            sample_rate: self.manifest.sample_rate,
        })
    }
    fn validate_new(&self, name: &RelativeName, role: &str) -> Result<AudioShape> {
        if self.active.is_some() {
            return Err(AnalysisError::Invalid("unfinished audio output"));
        }
        if self.artifacts.len() >= self.manifest.outputs.len() {
            return Err(AnalysisError::Budget("output count"));
        }
        if self
            .artifacts
            .iter()
            .any(|a| a.metadata.name.eq_ignore_ascii_case(&name.0) || a.metadata.role == role)
        {
            return Err(AnalysisError::Invalid("duplicate artifact name/role"));
        }
        if !name.0.ends_with(".wav") {
            return Err(AnalysisError::Invalid("output must be Float32 WAV"));
        }
        self.shape_for(role)
    }
    pub fn start_audio(&mut self, name: RelativeName, role: &str, work: &mut Work) -> Result<()> {
        work.checkpoint(1)?;
        let shape = self.validate_new(&name, role)?;
        let estimate = add(shape.pcm_bytes()?, 256)?;
        if add(self.used_disk, estimate)? > self.maximum_disk {
            return Err(AnalysisError::Budget("output disk"));
        }
        // The replacing codec writer only sees an exclusively owned directory.
        // Persistent destinations are handled separately with persist_noclobber.
        let writer = WavWriter::create(
            self.directory.path().join(&name.0),
            shape.sample_rate,
            shape.channels,
            WavSampleFormat::Float32,
        )?;
        self.active = Some(ActiveOutput {
            name,
            role: role.into(),
            shape,
            frames: 0,
            writer,
            estimate,
        });
        Ok(())
    }
    /// At most 4096 interleaved samples per call, with whole frames and finite
    /// values. The adapter cannot silently change duration or channel pairing.
    pub fn write_audio(&mut self, samples: &[f32], work: &mut Work) -> Result<()> {
        work.checkpoint(samples.len() as u64)?;
        let active = self
            .active
            .as_mut()
            .ok_or(AnalysisError::Invalid("no active output"))?;
        if samples.len() > CHUNK_SAMPLES
            || samples.is_empty()
            || !samples
                .len()
                .is_multiple_of(usize::from(active.shape.channels))
            || samples.iter().any(|x| !x.is_finite())
        {
            return Err(AnalysisError::Invalid("output chunk bounds/nonfinite"));
        }
        let frames = add(
            active.frames,
            (samples.len() / usize::from(active.shape.channels)) as u64,
        )?;
        if frames > active.shape.frames {
            return Err(AnalysisError::Invalid("output frame range"));
        }
        active.writer.write(samples)?;
        active.frames = frames;
        Ok(())
    }
    pub fn finish_audio(&mut self, work: &mut Work) -> Result<()> {
        work.checkpoint(1)?;
        let active = self
            .active
            .take()
            .ok_or(AnalysisError::Invalid("no active output"))?;
        if active.frames != active.shape.frames {
            return Err(AnalysisError::Invalid("unaligned output duration"));
        }
        active.writer.finalize()?;
        let path = self.directory.path().join(&active.name.0);
        let bytes = fs::metadata(&path)?.len();
        if bytes > active.estimate {
            return Err(AnalysisError::Budget("WAV overhead"));
        }
        validate_decoded(&path, active.shape, work)?;
        let fingerprint = ContentFingerprint::reader(File::open(&path)?, active.estimate, work)?;
        self.used_disk = add(self.used_disk, bytes)?;
        self.artifacts.push(Artifact {
            staged: path,
            metadata: ArtifactMetadata {
                name: active.name.0,
                role: active.role,
                shape: active.shape,
                frame_origin: self.origin,
                bytes,
                sha256: fingerprint.sha256,
                published_path: None,
            },
        });
        Ok(())
    }
    /// Bounded encoded adapter output is first copied to owned staging, then
    /// decoded and normalized to our checked Float32 WAV contract. Useful for
    /// future helper adapters; malformed codecs never become review artifacts.
    pub fn import_audio(
        &mut self,
        name: RelativeName,
        role: &str,
        mut reader: impl Read,
        work: &mut Work,
    ) -> Result<()> {
        let shape = self.validate_new(&name, role)?;
        let wav_reservation = add(shape.pcm_bytes()?, 256)?;
        let encoded_limit = self
            .maximum_disk
            .checked_sub(add(self.used_disk, wav_reservation)?)
            .ok_or(AnalysisError::Budget("encoded output disk"))?;
        let mut encoded = NamedTempFile::new_in(self.directory.path())?;
        let mut bytes = 0;
        let mut chunk = [0; CHUNK_BYTES];
        loop {
            work.check()?;
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            bytes = add(bytes, n as u64)?;
            if bytes > encoded_limit {
                return Err(AnalysisError::Budget("encoded output bytes"));
            }
            work.checkpoint(n as u64)?;
            encoded.write_all(&chunk[..n])?;
        }
        encoded.flush()?;
        work.check()?;
        // Probe before creating a decoder: other codec packet/runtime memory
        // cannot hide behind this fixed PCM verification reservation.
        let info = windfall_codec::probe_file(encoded.path())?;
        if info.format != "wave"
            || info.codec != "pcm_f32le"
            || info.sample_rate != shape.sample_rate
            || info.channels != shape.channels
            || info.frames != Some(shape.frames)
        {
            return Err(AnalysisError::Invalid(
                "encoded helper must be aligned Float32 WAV",
            ));
        }
        work.check()?;
        let decoded = windfall_codec::decode_file_strict_with(
            encoded.path(),
            &DecodeOptions {
                max_decoded_bytes: shape.pcm_bytes()?,
            },
        )?;
        if AudioShape::of(&decoded) != shape {
            return Err(AnalysisError::Invalid("decoded output shape"));
        }
        self.start_audio(name, role, work)?;
        for samples in decoded.samples().chunks(CHUNK_SAMPLES) {
            self.write_audio(samples, work)?;
        }
        // Drop the decode before finish_audio's independent verification decode.
        drop(decoded);
        drop(encoded);
        self.finish_audio(work)
    }
    pub(crate) fn complete(self, work: &mut Work) -> Result<(TempDir, Vec<Artifact>)> {
        work.check()?;
        if self.active.is_some() || self.artifacts.len() != self.manifest.outputs.len() {
            return Err(AnalysisError::Invalid("missing/unfinished model output"));
        }
        Ok((self.directory, self.artifacts))
    }
}

fn validate_decoded(path: &Path, shape: AudioShape, work: &mut Work) -> Result<()> {
    work.check()?;
    let (rate, channels, frames) = windfall_codec::decode_file_blocks::<AnalysisError>(
        path,
        &DecodeOptions {
            max_decoded_bytes: shape.pcm_bytes()?,
        },
        |_, _, samples| work.checkpoint(samples.len() as u64),
    )?;
    if (AudioShape {
        sample_rate: rate,
        channels,
        frames,
    }) != shape
    {
        return Err(AnalysisError::Invalid("output decode/alignment"));
    }
    Ok(())
}

pub(crate) fn publish(
    artifact: &Artifact,
    target: &Path,
    work: &mut Work,
    track: impl FnOnce(&Path),
) -> Result<()> {
    // Revalidate staging from the content before publication, and hash the
    // exact copied bytes. No failed/colliding publication ever unlinks target.
    let mut source = File::open(&artifact.staged)?;
    let mut file = NamedTempFile::new_in(
        target
            .parent()
            .ok_or(AnalysisError::Invalid("output parent"))?,
    )?;
    // Register immediately, before any fallible copy/check/rename. Destruction
    // is only a first cleanup attempt: the manager tracks a refused deletion.
    track(file.path());
    let mut sha = sha2::Sha256::new();
    use sha2::Digest;
    let mut bytes = 0;
    let mut chunk = [0; CHUNK_BYTES];
    loop {
        work.check()?;
        let n = source.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        bytes = add(bytes, n as u64)?;
        if bytes > artifact.metadata.bytes {
            return Err(AnalysisError::Checksum);
        }
        work.checkpoint(n as u64)?;
        file.write_all(&chunk[..n])?;
        sha.update(&chunk[..n]);
    }
    if bytes != artifact.metadata.bytes
        || <[u8; 32]>::from(sha.finalize()) != artifact.metadata.sha256
    {
        return Err(AnalysisError::Checksum);
    }
    file.as_file().sync_all()?;
    work.check()?;
    #[cfg(test)]
    PUBLICATION_TEST_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(file.path());
        }
    });
    match file.persist_noclobber(target) {
        Ok(file) => {
            drop(file);
            Ok(())
        }
        Err(e)
            if e.error.kind() == std::io::ErrorKind::AlreadyExists
                || fs::symlink_metadata(target).is_ok() =>
        {
            Err(AnalysisError::Collision)
        }
        Err(e) => Err(e.error.into()),
    }
}

#[cfg(test)]
type PublicationTestHook = Box<dyn FnMut(&Path)>;
#[cfg(test)]
thread_local! {
    pub(crate) static PUBLICATION_TEST_HOOK: std::cell::RefCell<Option<PublicationTestHook>> = const { std::cell::RefCell::new(None) };
}
