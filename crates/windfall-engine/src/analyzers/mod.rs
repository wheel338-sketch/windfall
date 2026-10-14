//! Bounded stereo PCM analysis, independent of device, document and UI ownership.
//!
//! [`prepare`] runs off audio/document locks. Only [`AudioTap::publish`],
//! [`AudioTap::reset`] and [`AudioTapSlot::boundary`] belong on audio. All
//! endpoints (including the slot) must ultimately be destroyed off audio.
//! Use the slot's retirement queue for replacement; never assign/drop taps
//! in a callback. See `docs/ANALYZER-TAPS.md` for math, limits and later hooks.

mod transport;
mod worker;

pub use transport::{AudioTap, AudioTapSlot, TapInstaller, prepare, tap_slot, usage};
pub use worker::{AnalyzerWorker, SnapshotReader, WorkerThread};

use std::fmt;
use windfall_project::{EffectId, TrackId};

pub const MAX_PUBLICATION_FRAMES: usize = 256;
pub const MAX_FFT_SIZE: usize = 4096;
pub const MAX_INPUT_PACKETS: usize = 64;
pub const MAX_SNAPSHOTS: usize = 8;
pub const SPECTROGRAM_SLICES: usize = 8;
pub const ENVELOPE_BINS: usize = 128;
pub const VECTOR_POINTS: usize = 128;
pub const MAX_LIVE_TAPS: usize = 8;
pub const MAX_TAP_SLOTS: usize = 8;
pub const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectGeneration(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionGeneration(pub u64);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockEpoch(pub u64);

/// Intended hook topology: one stereo location per prepared tap. Mix output
/// includes output gain and apart voices; track output follows effects/fader/pan.
/// Native engine hooks are a later integration gate, not installed by this module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    MixOutput,
    TrackPostFader(TrackId),
}

/// Caller-issued, non-reused selection ticket, including project replacement.
/// A prepared tap can never be relabeled with another selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub project: ProjectGeneration,
    pub generation: SelectionGeneration,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Rectangular,
    PeriodicHann,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub fft_size: usize,
    pub hop_size: usize,
    pub window: Window,
    pub input_packets: usize,
    pub snapshot_capacity: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            fft_size: 1024,
            hop_size: 256,
            window: Window::PeriodicHann,
            input_packets: 32,
            snapshot_capacity: 2,
        }
    }
}

impl Config {
    /// Pure control-side preflight of the conservative charged layout.
    pub fn layout(self) -> Result<Layout, PrepareError> {
        Ok(AnalyzerWorker::layout(self.validate()?))
    }

    pub fn validate(self) -> Result<Self, PrepareError> {
        if !(32..=MAX_FFT_SIZE).contains(&self.fft_size) || !self.fft_size.is_power_of_two() {
            return Err(PrepareError::FftSize);
        }
        if ![self.fft_size / 4, self.fft_size / 2, self.fft_size].contains(&self.hop_size) {
            return Err(PrepareError::HopSize);
        }
        if !(1..=MAX_INPUT_PACKETS).contains(&self.input_packets) {
            return Err(PrepareError::InputCapacity);
        }
        if !(1..=MAX_SNAPSHOTS).contains(&self.snapshot_capacity) {
            return Err(PrepareError::SnapshotCapacity);
        }
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareError {
    FftSize,
    HopSize,
    InputCapacity,
    SnapshotCapacity,
    InstanceLimit,
    SlotLimit,
    ByteLimit,
    Allocation,
}

impl fmt::Display for PrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let notice =
            windfall_core::notice!("engine.analyzers.refused", detail = format!("{self:?}"));
        write!(f, "{notice}")
    }
}
impl std::error::Error for PrepareError {}

/// Actual effect metadata only. Valid range is 0..=120 dB; invalid values
/// are counted and absent. No reduction is derived from PCM amplitude.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GainReduction {
    pub effect: EffectId,
    pub db: f32,
}

/// Clock of the first PCM frame, supplied by the real producer. Latencies
/// are explicit labels, each at most one second at this rate. None means
/// device latency is unknown, rather than zero.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stamp {
    pub selection: Selection,
    pub clock: ClockEpoch,
    pub first_frame: u64,
    pub sample_rate: u32,
    pub pdc_frames: u32,
    pub device_latency_frames: Option<u32>,
    pub gain_reduction: Option<GainReduction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Cancelled,
    WrongSelection,
    TooManyFrames,
    InvalidClock,
    FrameOverflow,
    QueueFull,
    EpochExhausted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// Explicit buffer/object payload; excludes opaque allocator bookkeeping.
    pub payload_bytes: usize,
    /// Payload plus conservative dependency/control overhead and a reserved
    /// 2 MiB native worker stack. The process quota charges this entire amount.
    pub reserved_bytes: usize,
    pub snapshot_pool: usize,
    pub input_frames: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub taps: usize,
    pub slots: usize,
    pub reserved_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counters {
    pub published_packets: u64,
    pub published_frames: u64,
    pub dropped_packets: u64,
    pub dropped_frames: u64,
    pub refused_publications: u64,
    pub invalid_samples: u64,
    pub invalid_metadata: u64,
    pub resets: u64,
    pub stale_packets: u64,
    pub stale_frames: u64,
    pub consumed_packets: u64,
    pub consumed_frames: u64,
    pub analyzed_windows: u64,
    pub dropped_snapshots: u64,
    pub stale_snapshots: u64,
    pub pool_faults: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PumpReport {
    pub packets: usize,
    pub snapshots: usize,
    pub stale_packets: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockRange {
    pub clock: ClockEpoch,
    pub first_frame: u64,
    pub end_frame: u64,
    pub sample_rate: u32,
    pub pdc_frames: u32,
    pub device_latency_frames: Option<u32>,
}

impl ClockRange {
    pub fn pdc_aligned_first_frame(self) -> i128 {
        i128::from(self.first_frame) - i128::from(self.pdc_frames)
    }

    /// An estimate only: caller-supplied device buffering is also removed.
    pub fn estimated_heard_first_frame(self) -> Option<i128> {
        self.device_latency_frames
            .map(|latency| self.pdc_aligned_first_frame() - i128::from(latency))
    }

    /// Off-thread big-clock label, retaining integer precision beyond 2^53.
    pub fn pdc_time_label(self) -> String {
        if self.sample_rate == 0 {
            return "Invalid sample rate".into();
        }
        let frames = self.pdc_aligned_first_frame();
        let millis = frames.unsigned_abs() * 1000 / u128::from(self.sample_rate);
        format!(
            "{}{:02}:{:02}:{:02}.{:03}",
            if frames < 0 { "-" } else { "" },
            millis / 3_600_000,
            millis / 60_000 % 60,
            millis / 1000 % 60,
            millis % 1000
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dbfs {
    Silence,
    Finite(f64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    /// Absolute PCM sample peak, not oversampled/intersample true peak.
    pub sample_peak: f64,
    pub rms: f64,
    pub peak_dbfs: Dbfs,
    /// Unit sample amplitude is the RMS reference (full-scale sine: -3.01 dB).
    pub rms_dbfs: Dbfs,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ChannelStats {
    pub invalid_samples: u32,
    pub clip_samples: u32,
    /// None if ANY sample in this channel's window is nonfinite.
    pub levels: Option<Levels>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StereoStats {
    /// Uncentered, normalized zero-lag cross product. None for silence or
    /// nonfinite PCM; this is not Pearson's DC-removed correlation.
    pub correlation: Option<f64>,
    pub mid_rms: Option<f64>,
    pub side_rms: Option<f64>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct SpectrumBin {
    pub coherent_amplitude: [f64; 2],
    pub mean_square: [f64; 2],
    pub phase_radians: [f64; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowCalibration {
    pub coherent_gain: f64,
    pub mean_square_gain: f64,
    pub enbw_bins: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnvelopeBin {
    pub first_frame: u64,
    pub end_frame: u64,
    pub min: [Option<f32>; 2],
    pub max: [Option<f32>; 2],
    pub invalid_samples: [u32; 2],
}

#[derive(Debug)]
pub struct SpectrogramSlice {
    pub range: ClockRange,
    pub valid: [bool; 2],
    pub mean_square: Vec<[f64; 2]>,
}

/// Borrowed from a reader; deliberately not Clone. All collections have
/// validated fixed capacities and belong to a finite preallocated pool.
#[derive(Debug)]
pub struct Snapshot {
    pub selection: Selection,
    pub stream_epoch: u64,
    pub range: ClockRange,
    pub config: Config,
    pub calibration: WindowCalibration,
    pub channels: [ChannelStats; 2],
    pub stereo: StereoStats,
    /// Invalid channel bins contain NaN, with valid=false. Do not serialize
    /// them as healthy zero data. Phase at zero magnitude is NaN (undefined).
    pub spectrum_valid: [bool; 2],
    pub spectrum: Vec<SpectrumBin>,
    /// Last at most 2*N contiguous raw frames, including nonfinite values.
    pub history_first_frame: u64,
    pub history: Vec<[f32; 2]>,
    pub envelope: Vec<EnvelopeBin>,
    /// Uniformly sampled (mid, side) points from the current window; points
    /// containing invalid PCM are omitted, and channel invalid counts remain.
    pub vectorscope: Vec<[f64; 2]>,
    /// Chronological last at most eight spectra from this contiguous segment.
    pub spectrogram: Vec<SpectrogramSlice>,
    pub spectrogram_count: usize,
    pub gain_reduction: Option<GainReduction>,
    pub invalid_metadata: u32,
    pub counters: Counters,
}

impl Snapshot {
    pub fn spectrogram_slices(&self) -> &[SpectrogramSlice] {
        &self.spectrogram[..self.spectrogram_count]
    }
}
