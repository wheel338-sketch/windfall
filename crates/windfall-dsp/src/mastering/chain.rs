//! Automatic mastering: brings a finished mix to a loudness target under a
//! true-peak ceiling, and says what it measured before and after.
//!
//! The chain is a gain that brings the mix to a reference loudness, three
//! band compressors whose depth is the one creative setting, a make-up gain,
//! and the [`MasterLimiter`] with its intersample detector. The make-up gain
//! is not known in advance, because the limiter takes back some of what it
//! is given. So the mix is run through the chain more than once: trial
//! passes that only measure, each correcting the gain from what the last
//! one came out at, and then one pass whose output is kept. A pass that
//! measures a true peak over the ceiling lowers the limiter's own ceiling
//! for the next, so the result is held to the target by measurement and not
//! by trusting the limiter.
//!
//! This is offline work. It allocates, and nothing here runs on the audio
//! thread.

use super::loudness::{LoudnessMeter, LoudnessMetrics};
use super::master_limiter::{MasterLimiter, MasterLimiterParams, master_limiter_latency};
use crate::blocks::math::db_to_gain;
use crate::effect::{Effect, GainReductionMeter};
use crate::multiband::{BandDynamicsParams, MultibandCompressor, MultibandCompressorParams};

/// Loudness targets a master can be asked for, in LUFS.
pub const MASTERING_TARGET_RANGE: (f32, f32) = (-30.0, -6.0);
/// True-peak ceilings a master can be asked for, in dBTP.
pub const MASTERING_CEILING_RANGE: (f32, f32) = (-9.0, 0.0);
/// How close to the target a master has to come to count as on target, in
/// LU either way.
pub const MASTERING_TOLERANCE_LU: f32 = 0.3;
/// How far over the ceiling a measured true peak may read, in dB.
pub const MASTERING_PEAK_TOLERANCE_DB: f32 = 0.05;

/// The loudness the mix is brought to before the band compressors, so that
/// their thresholds mean the same for a quiet mix and a loud one.
const REFERENCE_LUFS: f32 = -20.0;
/// Trial passes before the best one so far is kept.
const MAX_TRIALS: u32 = 6;
/// Frames run through the chain at a time.
const BLOCK: usize = 1_024;
static SILENCE: [f32; BLOCK * 2] = [0.0; BLOCK * 2];

/// What a master is asked to be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasteringSettings {
    /// Programme loudness to arrive at, in LUFS. -30 to -6, default -14.
    pub target_lufs: f32,
    /// The true peak nothing gets past, in dBTP. -9 to 0, default -1.
    pub ceiling_dbtp: f32,
    /// How firmly the three band compressors hold the mix together, 0 to
    /// 1, default 0.5. At 0 they are left out and only gain and limiting
    /// are applied.
    pub dynamics: f32,
}

impl Default for MasteringSettings {
    fn default() -> Self {
        Self {
            target_lufs: -14.0,
            ceiling_dbtp: -1.0,
            dynamics: 0.5,
        }
    }
}

impl MasteringSettings {
    /// A copy with every value forced into its range, and anything that is
    /// not a number replaced by the default.
    pub fn sanitized(&self) -> Self {
        let clean = |value: f32, (min, max): (f32, f32), fallback: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                fallback
            }
        };
        let defaults = Self::default();
        Self {
            target_lufs: clean(
                self.target_lufs,
                MASTERING_TARGET_RANGE,
                defaults.target_lufs,
            ),
            ceiling_dbtp: clean(
                self.ceiling_dbtp,
                MASTERING_CEILING_RANGE,
                defaults.ceiling_dbtp,
            ),
            dynamics: clean(self.dynamics, (0.0, 1.0), defaults.dynamics),
        }
    }
}

/// What a master did, for review.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MasteringReport {
    /// The mix as it came in.
    pub before: LoudnessMetrics,
    /// The master as it went out.
    pub after: LoudnessMetrics,
    /// The settings the master was made to, after sanitizing.
    pub settings: MasteringSettings,
    /// Gain applied ahead of the limiter, in dB: what brought the mix to
    /// the reference plus the make-up the passes settled on.
    pub gain_db: f32,
    /// The deepest reduction of any band compressor, in dB.
    pub compression_db: f32,
    /// The deepest reduction of the limiter, in dB.
    pub limiting_db: f32,
    /// Passes over the mix, trial passes and the kept one together.
    pub passes: u32,
    /// The master is within [`MASTERING_TOLERANCE_LU`] of the target and
    /// its true peak within [`MASTERING_PEAK_TOLERANCE_DB`] of the ceiling.
    /// False when the two cannot both be had, as with a loud target under a
    /// low ceiling: the master is then the nearest the passes found.
    pub target_reached: bool,
}

/// Why a mix cannot be mastered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MasteringError {
    /// Nothing in the mix passed the loudness gate.
    Silent,
}

impl std::fmt::Display for MasteringError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MasteringError::Silent => formatter
                .write_str("the mix is silent, so there is nothing to bring to a loudness target"),
        }
    }
}

impl std::error::Error for MasteringError {}

/// One trial's settings and how far it missed.
#[derive(Debug, Clone, Copy)]
struct Trial {
    trim_db: f32,
    limiter_ceiling_db: f32,
    loudness: f32,
    /// Smaller is better: the miss in LU, with a peak over the ceiling
    /// counted ten times as bad.
    score: f32,
    /// Positive when the measured peak exceeds the requested ceiling.
    peak_over_db: f32,
}

/// Masters a mix that is fed to it several times over.
///
/// ```text
/// let mut mastering = Mastering::new(48_000, settings);
/// for block in mix { mastering.analyze(block); }
/// mastering.begin()?;
/// let report = loop {
///     let keep = mastering.is_render_pass();
///     for block in mix {
///         mastering.process(block, &mut out);
///         if keep { write(&out); }
///     }
///     let report = mastering.finish_pass(&mut out);
///     if keep { write(&out); }
///     if let Some(report) = report { break report; }
/// };
/// ```
///
/// Every pass must be fed the same audio. What a pass puts out is as long
/// as what it was fed and lined up with it: the chain's latency is taken
/// off the front and comes out of [`Mastering::finish_pass`].
pub struct Mastering {
    sample_rate: u32,
    settings: MasteringSettings,
    source: LoudnessMeter,
    before: Option<LoudnessMetrics>,

    compressor: MultibandCompressor,
    compressor_meter: GainReductionMeter,
    limiter: MasterLimiter,
    limiter_meter: GainReductionMeter,
    /// Linear gain that brings the mix to the reference loudness.
    normalize: f32,
    normalize_db: f32,
    /// Make-up gain between the compressors and the limiter, in dB.
    trim_db: f32,
    trim: f32,
    /// The ceiling the limiter is set to, at or under the one asked for.
    limiter_ceiling_db: f32,

    output: LoudnessMeter,
    latency: usize,
    /// Frames of latency still to be dropped from the front of this pass.
    skip: usize,
    left: Vec<f32>,
    right: Vec<f32>,

    trials: u32,
    previous: Option<Trial>,
    best: Option<Trial>,
    render: bool,
}

impl Mastering {
    /// A master of audio at `sample_rate` (held to 8 kHz to 384 kHz) to
    /// `settings`, which are sanitized.
    pub fn new(sample_rate: u32, settings: MasteringSettings) -> Self {
        let sample_rate = sample_rate.clamp(8_000, 384_000);
        let settings = settings.sanitized();
        let compressor = MultibandCompressor::default();
        let limiter = MasterLimiter::default();
        Self {
            sample_rate,
            settings,
            source: LoudnessMeter::new(sample_rate),
            before: None,
            compressor_meter: compressor.meter(),
            compressor,
            limiter_meter: limiter.meter(),
            limiter,
            normalize: 1.0,
            normalize_db: 0.0,
            trim_db: 0.0,
            trim: 1.0,
            limiter_ceiling_db: settings.ceiling_dbtp,
            output: LoudnessMeter::new(sample_rate),
            latency: master_limiter_latency(sample_rate as f32),
            skip: 0,
            left: vec![0.0; BLOCK],
            right: vec![0.0; BLOCK],
            trials: 0,
            previous: None,
            best: None,
            render: false,
        }
    }

    /// The settings in force, after sanitizing.
    pub fn settings(&self) -> MasteringSettings {
        self.settings
    }

    /// Takes more of the mix as it is, to measure it: interleaved stereo.
    /// Feed all of it before [`Mastering::begin`].
    pub fn analyze(&mut self, interleaved: &[f32]) {
        self.source.feed(interleaved);
    }

    /// Ends the analysis and starts the first pass. Fails for a mix with
    /// no loudness to measure.
    pub fn begin(&mut self) -> Result<LoudnessMetrics, MasteringError> {
        let before = self.source.metrics();
        if !before.is_audible() {
            return Err(MasteringError::Silent);
        }
        self.before = Some(before);
        self.normalize_db = REFERENCE_LUFS - before.integrated_lufs;
        self.normalize = db_to_gain(self.normalize_db);
        // Were nothing in the chain to change the loudness, this would
        // land on the target at once.
        self.trim_db = self.settings.target_lufs - REFERENCE_LUFS;
        self.limiter_ceiling_db = self.settings.ceiling_dbtp;
        self.trials = 0;
        self.previous = None;
        self.best = None;
        self.render = false;

        let rate = self.sample_rate as f32;
        self.compressor.prepare(rate, BLOCK);
        self.limiter.prepare(rate, BLOCK);
        self.start_pass();
        Ok(before)
    }

    /// True for the pass whose output is the master. The passes before it
    /// only measure; what they put out is to be thrown away.
    pub fn is_render_pass(&self) -> bool {
        self.render
    }

    /// The band compressors' settings for a depth of 0 to 1. The mix is at
    /// the reference loudness when it reaches them.
    fn compression(dynamics: f32) -> MultibandCompressorParams {
        let band = |threshold_db: f32, ratio: f32, attack_ms: f32, release_ms: f32| {
            BandDynamicsParams {
                threshold_db,
                ratio: 1.0 + (ratio - 1.0) * dynamics,
                attack_ms,
                release_ms,
                makeup_db: 0.0,
            }
        };
        MultibandCompressorParams {
            low_crossover_hz: 160.0,
            high_crossover_hz: 3_200.0,
            low: band(-16.0, 2.5, 30.0, 250.0),
            mid: band(-16.0, 3.0, 20.0, 160.0),
            high: band(-24.0, 3.0, 8.0, 100.0),
            sidechain: false,
        }
    }

    fn start_pass(&mut self) {
        self.trim = db_to_gain(self.trim_db);
        self.compressor.reset();
        self.compressor
            .set_params(&Self::compression(self.settings.dynamics));
        self.limiter.reset();
        self.limiter.set_params(&MasterLimiterParams {
            gain_db: 0.0,
            leveling: 0.0,
            saturation: 0.0,
            tone: 0.0,
            ceiling_db: self.limiter_ceiling_db,
            release_ms: 150.0,
            true_peak: true,
        });
        // A reading left from the pass before is not this pass's.
        let _ = self.compressor_meter.take_db();
        let _ = self.limiter_meter.take_db();
        self.output.reset();
        self.skip = self.latency;
    }

    /// Runs up to [`BLOCK`] frames through the chain and appends what
    /// comes out, past the latency, to `out`.
    fn run(&mut self, chunk: &[f32], out: &mut Vec<f32>) {
        let frames = chunk.len() / 2;
        let left = &mut self.left[..frames];
        let right = &mut self.right[..frames];
        for (index, frame) in chunk.chunks_exact(2).enumerate() {
            let clean = |sample: f32| if sample.is_finite() { sample } else { 0.0 };
            left[index] = clean(frame[0]) * self.normalize;
            right[index] = clean(frame[1]) * self.normalize;
        }
        if self.settings.dynamics > 0.0 {
            self.compressor.process(left, right);
        }
        for sample in left.iter_mut().chain(right.iter_mut()) {
            *sample *= self.trim;
        }
        self.limiter.process(left, right);

        let skip = self.skip.min(frames);
        self.skip -= skip;
        let start = out.len();
        for index in skip..frames {
            out.push(left[index]);
            out.push(right[index]);
        }
        self.output.feed(&out[start..]);
    }

    /// Runs the next stretch of the mix through the chain. `out` is
    /// cleared and filled with the master of what has been fed so far,
    /// less the chain's latency: interleaved stereo.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        out.clear();
        for chunk in interleaved.chunks(BLOCK * 2) {
            self.run(chunk, out);
        }
    }

    /// Ends a pass. `out` is cleared and filled with the last of the
    /// master, the part the latency was holding back.
    ///
    /// After a trial pass this returns `None` and the next pass has begun:
    /// feed the mix again. After the render pass it returns the report.
    pub fn finish_pass(&mut self, out: &mut Vec<f32>) -> Option<MasteringReport> {
        out.clear();
        let mut remaining = self.latency;
        while remaining > 0 {
            let frames = remaining.min(BLOCK);
            self.run(&SILENCE[..frames * 2], out);
            remaining -= frames;
        }

        let after = self.output.metrics();
        let target = self.settings.target_lufs;
        let ceiling = self.settings.ceiling_dbtp;
        let miss = target - after.integrated_lufs;
        let over = after.true_peak_dbtp - ceiling;
        let reached =
            miss.abs() <= MASTERING_TOLERANCE_LU && over <= MASTERING_PEAK_TOLERANCE_DB;

        if self.render {
            let report = MasteringReport {
                before: self.before.unwrap_or(after),
                after,
                settings: self.settings,
                gain_db: self.normalize_db + self.trim_db,
                compression_db: self.compressor_meter.take_db(),
                limiting_db: self.limiter_meter.take_db(),
                passes: self.trials + 1,
                target_reached: reached,
            };
            return Some(report);
        }

        self.trials += 1;
        let trial = Trial {
            trim_db: self.trim_db,
            limiter_ceiling_db: self.limiter_ceiling_db,
            loudness: after.integrated_lufs,
            score: miss.abs() + 10.0 * over.max(0.0),
            peak_over_db: over,
        };
        // Peak safety is a constraint, not a preference that can be traded
        // for a smaller loudness miss. Among safe trials choose the closest
        // loudness. If none is safe yet, keep the best correction candidate.
        if self.best.is_none_or(|best| {
            let safe = trial.peak_over_db <= MASTERING_PEAK_TOLERANCE_DB;
            let best_safe = best.peak_over_db <= MASTERING_PEAK_TOLERANCE_DB;
            (safe && !best_safe) || (safe == best_safe && trial.score < best.score)
        }) {
            self.best = Some(trial);
        }

        if reached {
            // Run these very settings once more, and keep the output.
            self.render = true;
        } else if self.trials >= MAX_TRIALS {
            let best = self.best.unwrap_or(trial);
            self.trim_db = best.trim_db;
            self.limiter_ceiling_db = best.limiter_ceiling_db;
            if best.peak_over_db > MASTERING_PEAK_TOLERANCE_DB {
                self.limiter_ceiling_db = (self.limiter_ceiling_db - best.peak_over_db - 0.1)
                    .max(ceiling - 3.0);
            }
            self.render = true;
        } else {
            if over > MASTERING_PEAK_TOLERANCE_DB {
                // The limiter's ceiling goes down by what it let through.
                // It has three dB of room under the lowest ceiling asked.
                self.limiter_ceiling_db =
                    (self.limiter_ceiling_db - over - 0.05).max(ceiling - 3.0);
            }
            // How much of a dB of make-up comes out as loudness. A limiter
            // that is working takes some of it back, so judge by the last
            // two passes, and never assume less than a quarter.
            let yield_per_db = match self.previous {
                Some(previous) if (trial.trim_db - previous.trim_db).abs() > 0.05 => {
                    ((trial.loudness - previous.loudness) / (trial.trim_db - previous.trim_db))
                        .clamp(0.25, 1.0)
                }
                _ => 1.0,
            };
            self.previous = Some(trial);
            self.trim_db = (self.trim_db + miss / yield_per_db).clamp(-60.0, 60.0);
        }
        self.start_pass();
        None
    }
}

/// Masters a mix held in memory: interleaved stereo in, the master and its
/// report out. The master is as long as the mix and lined up with it.
pub fn master(
    interleaved: &[f32],
    sample_rate: u32,
    settings: &MasteringSettings,
) -> Result<(Vec<f32>, MasteringReport), MasteringError> {
    let mut mastering = Mastering::new(sample_rate, *settings);
    mastering.analyze(interleaved);
    mastering.begin()?;
    let mut block = Vec::new();
    loop {
        let keep = mastering.is_render_pass();
        let mut output = Vec::new();
        if keep {
            output.reserve(interleaved.len());
        }
        for piece in interleaved.chunks(BLOCK * 8) {
            mastering.process(piece, &mut block);
            if keep {
                output.extend_from_slice(&block);
            }
        }
        let report = mastering.finish_pass(&mut block);
        if keep {
            output.extend_from_slice(&block);
        }
        if let Some(report) = report {
            return Ok((output, report));
        }
    }
}
