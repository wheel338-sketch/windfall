//! Decoded audio files, kept so that the same file is not decoded twice.

use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use windfall_core::{AudioBuffer, AudioIdentity};
use windfall_ipc::SampleInfo;

use crate::paths;
use crate::sync::lock;

/// Buckets in the waveform overview of a [`SampleInfo`].
pub const PEAK_BUCKETS: usize = 512;

/// Decoded files the cache keeps at most.
const MAX_FILES: usize = 64;

/// Decoded audio the cache keeps at most, in bytes of samples. The file
/// decoded last is always kept, whatever its size.
const MAX_BYTES: usize = 256 * 1024 * 1024;

/// Waveform overviews kept before the oldest are forgotten.
const MAX_INFOS: usize = 512;

/// One version of one file: a file that is written again is a new entry.
#[derive(Debug, Clone)]
struct FileKey {
    path: PathBuf,
    identity: u64,
    modified: Option<SystemTime>,
    len: u64,
}

impl PartialEq for FileKey {
    fn eq(&self, other: &Self) -> bool {
        self.identity == other.identity
            && self.modified == other.modified
            && self.len == other.len
            && paths::same(&self.path, &other.path)
    }
}
impl Eq for FileKey {}

impl Hash for FileKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Match the shell's Windows ASCII case comparison without lossy
        // Unicode conversion. Other platforms retain Path's case semantics.
        #[cfg(windows)]
        {
            let bytes = self.path.as_os_str().as_encoded_bytes();
            bytes.len().hash(state);
            for byte in bytes {
                byte.to_ascii_lowercase().hash(state);
            }
        }
        #[cfg(not(windows))]
        self.path.hash(state);
        self.identity.hash(state);
        self.modified.hash(state);
        self.len.hash(state);
    }
}

impl FileKey {
    fn of(path: &Path) -> Result<Self, String> {
        let metadata = fs::metadata(path).map_err(|error| unreadable(path, &error))?;
        if metadata.is_dir() {
            return Err(format!(
                "\"{}\" is a folder, not an audio file",
                paths::display(path)
            ));
        }
        Ok(Self {
            path: path.to_path_buf(),
            identity: crate::library::file_identity(path).map_err(|e| unreadable(path, &e))?,
            modified: metadata.modified().ok(),
            len: metadata.len(),
        })
    }
}

fn unreadable(path: &Path, error: &dyn std::fmt::Display) -> String {
    format!("Could not load \"{}\": {error}", paths::display(path))
}

/// A small cache of decoded audio files and their waveform overviews.
///
/// The browser previews a file, asks for its waveform and then adds it to
/// the project; undo and redo take a sample out of the project and put it
/// back. All of those want the same decoded audio, and the audio itself is
/// shared, so keeping it costs nothing while the project also holds it.
///
/// Paths must be cleaned with [`paths::clean`] so one file has one entry.
#[derive(Default)]
pub struct SampleCache {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Least recently used first.
    decoded: Vec<(FileKey, AudioBuffer)>,
    /// Provenance survives LRU eviction while a project/worker still holds audio.
    /// Weak identities retain metadata only, and are pruned on the next decode.
    sources: HashMap<AudioIdentity, FileKey>,
    infos: HashMap<FileKey, SampleInfo>,
    /// Keys of `infos`, oldest first.
    info_order: Vec<FileKey>,
}

impl Inner {
    fn touch(&mut self, key: &FileKey) -> Option<AudioBuffer> {
        let index = self.decoded.iter().position(|(held, _)| held == key)?;
        let entry = self.decoded.remove(index);
        let buffer = entry.1.clone();
        self.decoded.push(entry);
        Some(buffer)
    }

    fn insert(&mut self, key: FileKey, buffer: AudioBuffer) {
        self.sources.retain(|identity, _| identity.is_live());
        self.sources.insert(buffer.identity(), key.clone());
        // An older version of the file is of no use once it has changed.
        self.decoded
            .retain(|(held, _)| !paths::same(&held.path, &key.path));
        self.decoded.push((key, buffer));
        let bytes = |entries: &[(FileKey, AudioBuffer)]| -> usize {
            entries
                .iter()
                .map(|(_, buffer)| buffer.retained_pcm_bytes())
                .sum()
        };
        while self.decoded.len() > 1
            && (self.decoded.len() > MAX_FILES || bytes(&self.decoded) > MAX_BYTES)
        {
            self.decoded.remove(0);
        }
    }
}

impl SampleCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Compares live decoded sources without filesystem work or audio copies.
    /// A separate decode after cache eviction may still represent the same file
    /// version. Unknown provenance is never assumed to be the current version.
    pub fn same_file_version(&self, a: &AudioBuffer, b: &AudioBuffer) -> bool {
        let a = a.identity();
        let b = b.identity();
        if a == b {
            return true;
        }
        let inner = lock(&self.inner);
        inner
            .sources
            .get(&a)
            .zip(inner.sources.get(&b))
            .is_some_and(|(a, b)| a == b)
    }

    /// Decodes the file at `path`, or returns the audio decoded earlier if
    /// the file has not changed since. Slow for a file not seen before; call
    /// it off the UI thread and without holding a lock.
    ///
    /// The error is a sentence that names the file and can be shown as it is.
    pub fn decode(&self, path: &Path) -> Result<AudioBuffer, String> {
        let key = FileKey::of(path)?;
        if let Some(buffer) = lock(&self.inner).touch(&key) {
            return Ok(buffer);
        }
        let buffer = windfall_codec::decode_file(path).map_err(|error| unreadable(path, &error))?;
        if FileKey::of(path)? != key {
            return Err("The source file changed while audio was being decoded. Try again.".into());
        }
        lock(&self.inner).insert(key, buffer.clone());
        Ok(buffer)
    }

    /// Bank workers reserve before decoding, including concurrent imports. Known
    /// frame counts are strict ceilings; unknown lengths use the remaining budget.
    pub fn decode_sample_bank(
        &self,
        path: &Path,
        budget: &std::sync::Arc<windfall_dsp::sample_bank::BankBudget>,
    ) -> Result<(AudioBuffer, windfall_dsp::sample_bank::BankReservation), String> {
        let key = FileKey::of(path)?;
        if let Some(buffer) = lock(&self.inner).touch(&key) {
            let reservation = budget.reserve(buffer.retained_pcm_bytes())?;
            return Ok((buffer, reservation));
        }
        // The .ds probe renders; do not let it allocate before our reservation.
        let frames = if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ds"))
        {
            None
        } else {
            let info = windfall_codec::probe_file(path).map_err(|e| unreadable(path, &e))?;
            info.frames
                .and_then(|frames| frames.checked_mul(u64::from(info.channels)))
                .and_then(|samples| samples.checked_mul(4))
        };
        // Four PCM ceilings cover the sink (including growth), decoder packet
        // and float conversion scratch. Unknown-length sources reserve the
        // remaining ledger and use one quarter of it as their decode ceiling.
        let available = budget.remaining();
        let bytes = frames.unwrap_or((available / 4) as u64);
        let bytes = usize::try_from(bytes).map_err(|_| "bank source byte overflow")?;
        if bytes > windfall_dsp::sample_bank::MAX_BANK_BYTES {
            return Err("bank source exceeds 256 MiB".into());
        }
        if bytes == 0 {
            return Err("Sample-bank memory refused: no decoder workspace remaining".into());
        }
        let mut reservation = budget.reserve(
            bytes
                .checked_mul(4)
                .ok_or("bank decoder workspace overflow")?,
        )?;
        let buffer = windfall_codec::decode_file_with(
            path,
            &windfall_codec::DecodeOptions {
                max_decoded_bytes: bytes as u64,
            },
        )
        .map_err(|e| unreadable(path, &e))?;
        if FileKey::of(path)? != key {
            return Err("The bank source changed while decoding; retry".into());
        }
        reservation.shrink(buffer.retained_pcm_bytes())?;
        lock(&self.inner).insert(key, buffer.clone());
        Ok((buffer, reservation))
    }

    /// The audio last decoded from `path`, if it is still held. Does not
    /// touch the disk, so it is safe where waiting is not: it may return
    /// audio from before the file last changed.
    pub fn peek(&self, path: &Path) -> Option<AudioBuffer> {
        let inner = lock(&self.inner);
        let (_, buffer) = inner
            .decoded
            .iter()
            .rev()
            .find(|(key, _)| key.path == path)?;
        Some(buffer.clone())
    }

    /// Facts about the file at `path` and its waveform overview. Decodes the
    /// file the first time, so the same rules as [`decode`](Self::decode)
    /// apply.
    pub fn info(&self, path: &Path) -> Result<SampleInfo, String> {
        let key = FileKey::of(path)?;
        if let Some(info) = lock(&self.inner).infos.get(&key) {
            let mut info = info.clone();
            // The waveform is shared by aliases; facts still describe the
            // spelling the caller selected (including a case-only rename).
            info.path = paths::display(path);
            info.name = paths::stem(path);
            return Ok(info);
        }
        let buffer = self.decode(path)?;
        let source_warnings = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ds"))
        {
            windfall_codec::render_drumsynth_file(path, &windfall_codec::DecodeOptions::default())
                .map_err(|error| unreadable(path, &error))?
                .warnings
        } else {
            Vec::new()
        };
        if FileKey::of(path)? != key {
            return Err(
                "The source file changed while its information was being read. Try again.".into(),
            );
        }
        let info = SampleInfo {
            path: paths::display(path),
            name: paths::stem(path),
            sample_rate: buffer.sample_rate(),
            channels: buffer.channels(),
            frames: buffer.frames() as u64,
            duration_secs: buffer.duration_secs(),
            peaks: windfall_codec::peaks(&buffer, PEAK_BUCKETS),
            source_warnings,
        };

        let mut inner = lock(&self.inner);
        if inner.infos.insert(key.clone(), info.clone()).is_none() {
            inner.info_order.push(key);
        }
        while inner.info_order.len() > MAX_INFOS {
            let oldest = inner.info_order.remove(0);
            inner.infos.remove(&oldest);
        }
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use windfall_codec::{WavSampleFormat, write_wav};

    use super::*;

    fn write_tone(path: &Path, frames: usize, level: f32) {
        let buffer = AudioBuffer::from_interleaved(48_000, 1, vec![level; frames]);
        write_wav(path, &buffer, WavSampleFormat::Float32).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn case_alias_reuses_cached_audio_and_reports_the_requested_info_path() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Tone.wav");
        let alias = folder.path().join("tOnE.WAV");
        write_tone(&file, 100, 0.25);
        let cache = SampleCache::new();
        let first = cache.decode(&file).unwrap();
        cache.info(&file).unwrap();
        let second = cache.decode(&alias).unwrap();
        assert_eq!(
            first.identity(),
            second.identity(),
            "unchanged Windows alias should reuse cached audio"
        );
        let info = cache.info(&alias).unwrap();
        assert_eq!(info.path, paths::display(&alias));
        assert_eq!(info.name, paths::stem(&alias));
        assert_eq!(cache.inner.lock().unwrap().decoded.len(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn case_alias_live_provenance_survives_rename_and_cache_eviction() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Tone.wav");
        let alias = folder.path().join("tOnE.WAV");
        write_tone(&file, 100, 0.25);
        let cache = SampleCache::new();
        let first = cache.decode(&file).unwrap();
        fs::rename(&file, &alias).unwrap();
        for i in 0..=MAX_FILES {
            let other = folder.path().join(format!("evict-{i}.wav"));
            write_tone(&other, 8, 0.1);
            cache.decode(&other).unwrap();
        }
        assert!(cache.peek(&file).is_none());
        let second = cache.decode(&alias).unwrap();
        assert_ne!(first.identity(), second.identity());
        assert!(
            cache.same_file_version(&first, &second),
            "case-only rename must retain live source provenance after eviction"
        );
        write_tone(&alias, 200, 0.75);
        let changed = cache.decode(&alias).unwrap();
        assert!(!cache.same_file_version(&first, &changed));
        assert_eq!(changed.samples()[0], 0.75);
    }

    #[test]
    fn case_alias_versions_require_identity_size_time_and_platform_paths() {
        use std::hash::{Hash, Hasher};
        let original = FileKey {
            path: PathBuf::from("C:/Samples/Tone.wav"),
            identity: 42,
            modified: Some(SystemTime::UNIX_EPOCH),
            len: 100,
        };
        let alias = FileKey {
            path: PathBuf::from("c:/samples/tone.wav"),
            ..original.clone()
        };
        assert_eq!(original == alias, cfg!(windows));
        if cfg!(windows) {
            let hash = |key: &FileKey| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut hasher);
                hasher.finish()
            };
            assert_eq!(hash(&original), hash(&alias));
        }
        assert_ne!(
            original,
            FileKey {
                identity: 43,
                ..alias.clone()
            }
        );
        assert_ne!(
            original,
            FileKey {
                len: 101,
                ..alias.clone()
            }
        );
        assert_ne!(
            original,
            FileKey {
                modified: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1)),
                ..alias
            }
        );
    }

    #[test]
    fn live_source_versions_survive_eviction_but_do_not_retain_audio() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("tone.wav");
        write_tone(&file, 100, 0.25);
        let cache = SampleCache::new();
        let first = cache.decode(&file).unwrap();
        let identity = first.identity();
        for i in 0..MAX_FILES {
            let other = folder.path().join(format!("{i}.wav"));
            write_tone(&other, 100, 0.25);
            cache.decode(&other).unwrap();
        }
        assert!(cache.peek(&file).is_none());
        let again = cache.decode(&file).unwrap();
        assert_ne!(identity, again.identity());
        assert!(cache.same_file_version(&first, &again));
        write_tone(&file, 200, 0.75);
        let changed = cache.decode(&file).unwrap();
        assert!(!cache.same_file_version(&first, &changed));
        assert!(!cache.same_file_version(
            &AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 100]),
            &changed
        ));
        drop(first);
        assert!(!identity.is_live());
        // Another new decode prunes expired source metadata.
        let other = folder.path().join("last.wav");
        write_tone(&other, 100, 0.25);
        cache.decode(&other).unwrap();
        assert!(!lock(&cache.inner).sources.contains_key(&identity));
    }

    #[test]
    fn a_file_is_decoded_once_until_it_changes() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("tone.wav");
        write_tone(&file, 100, 0.25);
        let cache = SampleCache::new();

        let first = cache.decode(&file).unwrap();
        let second = cache.decode(&file).unwrap();
        assert!(std::ptr::eq(first.samples(), second.samples()));
        assert!(std::ptr::eq(
            cache.peek(&file).unwrap().samples(),
            first.samples()
        ));

        // A different length is a different file, even within the clock's
        // resolution.
        write_tone(&file, 200, 0.5);
        let third = cache.decode(&file).unwrap();
        assert_eq!(third.frames(), 200);
        assert_eq!(cache.peek(&file).unwrap().frames(), 200);
    }

    #[test]
    fn replacing_a_file_with_the_same_size_and_timestamp_invalidates_decoded_audio() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("tone.wav");
        let replacement = folder.path().join("replacement.wav");
        write_tone(&file, 100, 0.25);
        let cache = SampleCache::new();
        let first = cache.decode(&file).unwrap();
        let modified = fs::metadata(&file).unwrap().modified().unwrap();
        write_tone(&replacement, 100, 0.75);
        fs::OpenOptions::new()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
        fs::remove_file(&file).unwrap();
        fs::rename(&replacement, &file).unwrap();
        let second = cache.decode(&file).unwrap();
        assert_eq!(first.samples()[0], 0.25);
        assert_eq!(second.samples()[0], 0.75);
        assert!(!std::ptr::eq(first.samples(), second.samples()));
    }

    #[test]
    fn info_describes_the_file_and_is_kept() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Tone One.wav");
        write_tone(&file, 4_800, 0.5);
        let cache = SampleCache::new();

        let info = cache.info(&file).unwrap();
        assert_eq!(info.name, "Tone One");
        assert_eq!(info.path, paths::display(&file));
        assert_eq!(
            (info.sample_rate, info.channels, info.frames),
            (48_000, 1, 4_800)
        );
        assert!((info.duration_secs - 0.1).abs() < 1e-9);
        assert_eq!(info.peaks.len(), PEAK_BUCKETS * 2);
        assert!(info.peaks.iter().all(|peak| (peak - 0.5).abs() < 1e-6));
        assert_eq!(cache.info(&file).unwrap(), info);
    }

    #[test]
    fn a_file_that_cannot_be_read_gives_a_sentence_that_names_it() {
        let folder = tempfile::tempdir().unwrap();
        let cache = SampleCache::new();

        let missing = folder.path().join("missing.wav");
        let error = cache.decode(&missing).unwrap_err();
        assert!(error.starts_with("Could not load \""), "{error}");
        assert!(error.contains("missing.wav"), "{error}");

        let text = folder.path().join("notes.wav");
        fs::write(&text, "not audio at all").unwrap();
        let error = cache.info(&text).unwrap_err();
        assert!(error.contains("notes.wav"), "{error}");
        assert!(error.contains("not a supported audio file"), "{error}");

        let error = cache.decode(folder.path()).unwrap_err();
        assert!(error.contains("is a folder"), "{error}");
        assert!(cache.peek(&text).is_none());
    }

    #[test]
    fn the_cache_forgets_the_least_recently_used_file() {
        let folder = tempfile::tempdir().unwrap();
        let cache = SampleCache::new();
        let file = |index: usize| folder.path().join(format!("{index}.wav"));
        for index in 0..=MAX_FILES {
            write_tone(&file(index), 8, 0.1);
            cache.decode(&file(index)).unwrap();
            // Keeps the first file the most recently used.
            cache.decode(&file(0)).unwrap();
        }
        assert!(cache.peek(&file(0)).is_some());
        assert!(cache.peek(&file(1)).is_none());
        assert!(cache.peek(&file(MAX_FILES)).is_some());
    }
}

#[cfg(test)]
mod bank_budget_tests {
    use super::*;
    #[test]
    fn bank_decoder_refuses_before_allocating_pcm_and_releases_workspace_on_control() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bank.wav");
        let audio = AudioBuffer::from_interleaved(48000, 1, vec![0.2; 16]);
        windfall_codec::write_wav(&path, &audio, windfall_codec::WavSampleFormat::Float32).unwrap();
        let budget = windfall_dsp::sample_bank::BankBudget::new(256);
        let budget = std::sync::Arc::new(budget);
        let cache = SampleCache::new();
        let pending = budget.reserve(1).unwrap();
        assert!(
            cache
                .decode_sample_bank(&path, &budget)
                .unwrap_err()
                .contains("memory refused")
        );
        assert!(
            cache.peek(&path).is_none(),
            "refused decoder did not allocate/cache PCM"
        );
        drop(pending);
        let (decoded, workspace) = cache.decode_sample_bank(&path, &budget).unwrap();
        assert_eq!(decoded.samples(), audio.samples());
        assert_eq!(budget.used(), 64);
        assert!(budget.reserve(1).is_err());
        drop(workspace);
        assert_eq!(budget.used(), 0);
    }
}
