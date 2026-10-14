//! Bounded worker-side sampler banks. No lookup or rendering happens on note-on.
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use windfall_core::AudioBuffer;
use windfall_project::{ClipStretchQuality, SamplerSettings, SamplerStretch};
use windfall_stretch::{
    MAX_PITCH_SEMITONES, Quality, stretch_with_formants_cancellable, stretched_frames,
};

/// Aggregate retained source/variant bytes, including unpublished reservations.
pub const SAMPLER_BYTE_BUDGET: usize = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SamplerPreparationError {
    Cancelled,
    Budget {
        requested: usize,
        retained: usize,
        limit: usize,
    },
    Unsupported(&'static str),
    Asset(String),
}
impl std::fmt::Display for SamplerPreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let notice = match self {
            Self::Cancelled => windfall_core::notice!("engine.sampler.cancelled"),
            Self::Budget {
                requested,
                retained,
                limit,
            } => windfall_core::notice!(
                "engine.sampler.budget",
                requested = requested,
                retained = retained,
                limit = limit
            ),
            Self::Unsupported(reason) => windfall_core::notice::term(reason),
            Self::Asset(reason) => windfall_core::notice::term(reason),
        };
        write!(f, "{notice}")
    }
}
impl std::error::Error for SamplerPreparationError {}

#[derive(Debug)]
pub(crate) struct Budget {
    used: AtomicUsize,
    limit: usize,
    /// One FFT workspace per shared ledger, including replacement/export jobs.
    rendering: Mutex<()>,
}
impl Budget {
    fn render_guard(
        &self,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<MutexGuard<'_, ()>, SamplerPreparationError> {
        loop {
            if !keep_going() {
                return Err(SamplerPreparationError::Cancelled);
            }
            match self.rendering.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
    }
}
#[derive(Debug)]
struct Lease {
    budget: Arc<Budget>,
    bytes: usize,
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
impl Lease {
    fn reserve(budget: &Arc<Budget>, bytes: usize) -> Result<Self, SamplerPreparationError> {
        let mut used = budget.used.load(Ordering::Acquire);
        loop {
            if bytes > budget.limit.saturating_sub(used) {
                return Err(SamplerPreparationError::Budget {
                    requested: bytes,
                    retained: used,
                    limit: budget.limit,
                });
            }
            match budget.used.compare_exchange_weak(
                used,
                used + bytes,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(Self {
                        budget: budget.clone(),
                        bytes,
                    });
                }
                Err(now) => used = now,
            }
        }
    }
    fn shrink(&mut self, bytes: usize) {
        self.budget
            .used
            .fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Key {
    stretch: SamplerStretch,
    root: u8,
    tune: f32,
    start: usize,
    end: usize,
    reverse: bool,
}

/// Voices retain the bank, so its reservation lives as long as any variant does.
#[derive(Debug)]
pub(crate) struct SamplerBank {
    source: AudioBuffer,
    key: Key,
    pub frames: usize,
    pub variants: [Option<AudioBuffer>; 128],
    _lease: Lease,
}
impl SamplerBank {
    pub fn at(&self, key: u8) -> Option<&AudioBuffer> {
        self.variants.get(usize::from(key))?.as_ref()
    }
    pub fn holds(&self, audio: &AudioBuffer) -> bool {
        self.variants
            .iter()
            .flatten()
            .any(|other| crate::plan::same_audio(other, audio))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SamplerAudioCache {
    pub budget: Arc<Budget>,
    entries: VecDeque<Arc<SamplerBank>>,
}
impl Default for SamplerAudioCache {
    fn default() -> Self {
        Self::with_limit(SAMPLER_BYTE_BUDGET)
    }
}
impl SamplerAudioCache {
    pub fn with_limit(limit: usize) -> Self {
        Self {
            budget: Arc::new(Budget {
                used: AtomicUsize::new(0),
                limit: limit.min(SAMPLER_BYTE_BUDGET),
                rendering: Mutex::new(()),
            }),
            entries: VecDeque::new(),
        }
    }
    pub fn fresh(&self) -> Self {
        Self {
            budget: self.budget.clone(),
            entries: VecDeque::new(),
        }
    }
    pub fn used(&self) -> usize {
        self.budget.used.load(Ordering::Acquire)
    }
    pub fn get(
        &self,
        source: &AudioBuffer,
        settings: &SamplerSettings,
    ) -> Option<Arc<SamplerBank>> {
        let key = key(source, settings);
        self.entries
            .iter()
            .find(|entry| entry.key == key && crate::plan::same_audio(&entry.source, source))
            .cloned()
    }
    pub fn insert(&mut self, bank: Arc<SamplerBank>) {
        self.entries.push_back(bank);
    }
    pub fn prune(
        &mut self,
        project: &windfall_project::Project,
        source: impl Fn(windfall_project::SampleId) -> Option<AudioBuffer>,
    ) {
        self.entries.retain(|bank| {
            project.channels.iter().any(|channel| {
                let windfall_project::ChannelSource::Sampler(settings) = &channel.source else {
                    return false;
                };
                settings.sample.and_then(&source).is_some_and(|audio| {
                    key(&audio, settings) == bank.key
                        && crate::plan::same_audio(&audio, &bank.source)
                })
            })
        });
    }
}

/// Rounding agrees with tape trim. Every nonempty selection has at least one frame.
pub(crate) fn trim(source: &AudioBuffer, settings: &SamplerSettings) -> (usize, usize) {
    let frames = source.frames();
    if frames == 0 {
        return (0, 0);
    }
    let start = ((f64::from(settings.start.clamp(0.0, 1.0)) * frames as f64).round() as usize)
        .min(frames - 1);
    let end = ((f64::from(settings.end.clamp(0.0, 1.0)) * frames as f64).round() as usize)
        .clamp(start + 1, frames);
    (start, end)
}
fn key(source: &AudioBuffer, settings: &SamplerSettings) -> Key {
    let (start, end) = trim(source, settings);
    Key {
        stretch: settings.stretch,
        root: settings.root_key,
        tune: settings.tune,
        start,
        end,
        reverse: settings.reverse,
    }
}

pub(crate) fn prepare(
    source: &AudioBuffer,
    settings: &SamplerSettings,
    budget: &Arc<Budget>,
    keep_going: &mut dyn FnMut() -> bool,
    progress: &mut dyn FnMut(u8, u8),
) -> Result<Arc<SamplerBank>, SamplerPreparationError> {
    let SamplerStretch::Spectral {
        ratio,
        quality,
        formants,
        range,
    } = settings.stretch
    else {
        return Err(SamplerPreparationError::Unsupported(
            "A tape sampler does not need preparation.",
        ));
    };
    settings
        .stretch
        .validate()
        .map_err(SamplerPreparationError::Unsupported)?;
    // Bound FFT working storage as well as the derived audio. No downmix surprise.
    if source.channels() > 2 || !(8_000..=192_000).contains(&source.sample_rate()) {
        return Err(SamplerPreparationError::Unsupported(
            "Spectral samplers support mono/stereo sources at 8–192 kHz.",
        ));
    }
    if !settings.tune.is_finite() || settings.tune.abs() > 48.0 || settings.root_key > 127 {
        return Err(SamplerPreparationError::Unsupported(
            "The sampler pitch settings are invalid.",
        ));
    }
    if !keep_going() {
        return Err(SamplerPreparationError::Cancelled);
    }
    let key = key(source, settings);
    let trimmed = key.end - key.start;
    let frames = stretched_frames(trimmed, ratio).max(1);
    let keys = usize::from(range.last - range.first) + 1;
    let bytes = |frames: usize| {
        frames
            .saturating_mul(usize::from(source.channels()))
            .saturating_mul(4)
    };
    let live = bytes(frames)
        .saturating_mul(keys)
        .saturating_add(source.samples().len().saturating_mul(4))
        .saturating_add(std::mem::size_of::<SamplerBank>());
    // Count trim and intermediate audio while rendering too. FFT storage is
    // separately bounded by stereo/192 kHz and the three fixed quality presets.
    let staging = live
        .saturating_add(bytes(trimmed).saturating_mul(3))
        .saturating_add(bytes(frames).saturating_mul(2));
    let mut lease = Lease::reserve(budget, staging)?;
    // Off-lock workers serialize FFT construction/processing. Its fixed working
    // storage cannot multiply with overlapping Apply/open/reload/export jobs.
    let _rendering = budget.render_guard(keep_going)?;
    let channels = usize::from(source.channels());
    let mut data = source.samples()[key.start * channels..key.end * channels].to_vec();
    if settings.reverse {
        for i in 0..trimmed / 2 {
            for c in 0..channels {
                data.swap(i * channels + c, (trimmed - 1 - i) * channels + c);
            }
        }
    }
    let input = AudioBuffer::from_interleaved(source.sample_rate(), source.channels(), data);
    let quality = match quality {
        ClipStretchQuality::Fast => Quality::Fast,
        ClipStretchQuality::Standard => Quality::Standard,
        ClipStretchQuality::High => Quality::High,
    };
    let mut variants = std::array::from_fn(|_| None);
    for note in range.first..=range.last {
        if !keep_going() {
            return Err(SamplerPreparationError::Cancelled);
        }
        let mut pitch = f64::from(note) - f64::from(settings.root_key) + f64::from(settings.tune);
        let mut rendered = input.clone();
        // At most eight passes for all validated MIDI/root/tuning combinations.
        while pitch.abs() > MAX_PITCH_SEMITONES {
            let shift = pitch.signum() * MAX_PITCH_SEMITONES;
            rendered = stretch_with_formants_cancellable(
                &rendered, 1.0, shift, quality, formants, keep_going,
            )
            .ok_or(SamplerPreparationError::Cancelled)?;
            pitch -= shift;
        }
        // Rounding a one-frame contraction to zero would lose a held loop.
        let effective_ratio = frames as f64 / trimmed.max(1) as f64;
        rendered = stretch_with_formants_cancellable(
            &rendered,
            effective_ratio,
            pitch,
            quality,
            formants,
            keep_going,
        )
        .ok_or(SamplerPreparationError::Cancelled)?;
        variants[usize::from(note)] = Some(rendered);
        progress(note - range.first + 1, range.last - range.first + 1);
    }
    drop(input);
    lease.shrink(live);
    Ok(Arc::new(SamplerBank {
        source: source.clone(),
        key,
        frames,
        variants,
        _lease: lease,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::SamplerKeyRange;

    #[test]
    fn retained_source_prevents_pointer_reuse_matching_a_stale_bank() {
        let source = AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 512]);
        let identity = source.samples().as_ptr();
        let settings = SamplerSettings {
            stretch: SamplerStretch::Spectral {
                ratio: 1.0,
                quality: ClipStretchQuality::Fast,
                formants: false,
                range: SamplerKeyRange {
                    first: 60,
                    last: 60,
                },
            },
            ..Default::default()
        };
        let mut cache = SamplerAudioCache::default();
        let bank = prepare(
            &source,
            &settings,
            &cache.budget,
            &mut || true,
            &mut |_, _| {},
        )
        .unwrap();
        cache.insert(bank.clone());
        drop(source);
        assert_eq!(bank.source.samples().as_ptr(), identity);
        assert_eq!(bank.source.samples(), &[0.25; 512]);
        let replacement = AudioBuffer::from_interleaved(48_000, 1, vec![0.5; 512]);
        assert_ne!(replacement.samples().as_ptr(), identity);
        assert!(cache.get(&replacement, &settings).is_none());
    }

    #[test]
    fn aggregate_reservation_has_no_oversized_exception_and_releases_on_drop() {
        let cache = SamplerAudioCache::with_limit(4096);
        let first = Lease::reserve(&cache.budget, 3000).unwrap();
        let second = Lease::reserve(&cache.budget, 1096).unwrap();
        assert_eq!(cache.used(), 4096);
        assert!(matches!(
            Lease::reserve(&cache.budget, 1),
            Err(SamplerPreparationError::Budget {
                retained: 4096,
                limit: 4096,
                ..
            })
        ));
        drop(first);
        drop(second);
        assert_eq!(cache.used(), 0);
        assert!(Lease::reserve(&cache.budget, 4097).is_err());
        assert_eq!(cache.used(), 0);
    }

    #[test]
    fn queued_preparation_cancels_before_allocating_a_second_fft_workspace() {
        let cache = SamplerAudioCache::default();
        let _rendering = cache.budget.rendering.lock().unwrap();
        let source = AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 512]);
        let settings = SamplerSettings {
            stretch: SamplerStretch::Spectral {
                ratio: 2.0,
                quality: ClipStretchQuality::High,
                formants: false,
                range: SamplerKeyRange {
                    first: 60,
                    last: 60,
                },
            },
            ..Default::default()
        };
        let mut checks = 0;
        assert!(matches!(
            prepare(
                &source,
                &settings,
                &cache.budget,
                &mut || {
                    checks += 1;
                    checks < 3
                },
                &mut |_, _| panic!("queued preparation must not render")
            ),
            Err(SamplerPreparationError::Cancelled)
        ));
        assert_eq!(checks, 3);
        assert_eq!(cache.used(), 0);
    }
}
