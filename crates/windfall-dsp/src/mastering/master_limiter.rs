//! Mastering Limiter: three stages in series that share the work of making
//! a mix louder, so that no one of them has to do it all.
//!
//! 1. **Level** rides the average level slowly. It takes the bulk of the
//!    reduction off sustained loudness and leaves the shape of each hit
//!    alone, which is what keeps the dynamics.
//! 2. **Color** rounds what still stands out with the saturation curve of
//!    [`StageStack`](super::StageStack), blended in by an amount.
//! 3. **Peak** is a look-ahead brickwall that reads the waveform between
//!    the samples, so it holds the true peak and not just the sample peak.
//!
//! Every stage applies one gain to the whole band and to both channels, so
//! the tonal balance and the stereo image of the mix are those it came in
//! with. [`StageStack`](super::StageStack) is the same idea reduced to its
//! second stage and a plain safety clip.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::loudness::{PEAK_DELAY, PeakInterpolator};
use crate::balance::{Controls, audio, rate};
use crate::blocks::dc::DcBlocker;
use crate::blocks::envelope::{Ballistics, MeanSquare};
use crate::blocks::math::{
    db_to_gain, db_to_gain_exp, level_to_db, ms_to_samples, smoothing_coefficient,
};
use crate::effect::{Effect, GainReductionMeter};
use crate::param::{ParamSet, param_set};

/// How far ahead the peak stage sees, in ms. Fixed, so the latency depends
/// on the sample rate alone.
const LOOKAHEAD_MS: f32 = 1.5;

/// Length of every ring: a power of two above the longest look-ahead (576
/// samples at 384 kHz) plus the detector's delay.
const RING: usize = 1024;
const MASK: usize = RING - 1;

/// The level stage: RMS over this window, measured against this threshold.
const LEVEL_WINDOW_MS: f32 = 200.0;
pub const LEVEL_THRESHOLD_DB: f32 = -16.0;
const LEVEL_KNEE_DB: f32 = 6.0;
/// The share of every dB over the threshold the level stage takes away
/// with Leveling at its top: 2.5 to 1.
pub const LEVEL_MAX_SLOPE: f32 = 0.6;
const LEVEL_ATTACK_MS: f32 = 80.0;
const LEVEL_RELEASE_MS: f32 = 800.0;

/// Corner of the filter that keeps the color stage from adding an offset.
const COLOR_DC_HZ: f32 = 10.0;

/// The latency of a [`MasterLimiter`] at `sample_rate`: its look-ahead plus
/// the delay of its intersample detector. No setting changes it.
pub fn master_limiter_latency(sample_rate: f32) -> usize {
    ms_to_samples(LOOKAHEAD_MS, rate(sample_rate)) as usize + PEAK_DELAY
}

/// Settings of the [`MasterLimiter`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MasterLimiterParams {
    /// Gain into the stages, in dB. 0 to 24, default 0. This is the
    /// loudness control: the stages decide how the extra level is held.
    pub gain_db: f32,
    /// How much of the work the slow level stage takes, 0 to 1, default
    /// 0.5. At 0 the peak stage does everything, which is loudest and
    /// flattest. At 1 sustained level over -16 dB RMS is eased down at
    /// 2.5 to 1 before the peaks are touched.
    pub leveling: f32,
    /// How much of the saturation curve is blended in, 0 to 1, default 0.
    pub saturation: f32,
    /// Harmonic color of the saturation, 0 (odd only) to 1 (added even
    /// harmonics), default 0.
    pub tone: f32,
    /// The level nothing gets past, in dB relative to full scale. -12 to
    /// 0, default -1.
    pub ceiling_db: f32,
    /// How quickly the peak stage recovers: the time to complete 63% of
    /// the recovery, in ms. 10 to 1000, default 150.
    pub release_ms: f32,
    /// Holds the peak of the waveform between the samples to the ceiling,
    /// not only the samples. Default on.
    pub true_peak: bool,
}

impl Default for MasterLimiterParams {
    fn default() -> Self {
        Self {
            gain_db: 0.0,
            leveling: 0.5,
            saturation: 0.0,
            tone: 0.0,
            ceiling_db: -1.0,
            release_ms: 150.0,
            true_peak: true,
        }
    }
}

param_set!(MasterLimiterParams, "Mastering Limiter", {
    float [gain_db] "gainDb" "Gain" { Decibels, Linear, 0.0, 24.0, 0.0 }
    float [leveling] "leveling" "Leveling" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [saturation] "saturation" "Saturation" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [tone] "tone" "Odd/even tone" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [ceiling_db] "ceilingDb" "Ceiling" { Decibels, Linear, -12.0, 0.0, -1.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 10.0, 1000.0, 150.0 }
    toggle [true_peak] "truePeak" "True peak" { true }
});

/// Reduction in dB of the level stage for an average level `over_db` above
/// its threshold, with a soft knee.
#[inline]
pub fn level_reduction_db(over_db: f32, slope: f32) -> f32 {
    if 2.0 * over_db <= -LEVEL_KNEE_DB {
        0.0
    } else if 2.0 * over_db < LEVEL_KNEE_DB {
        let into_knee = over_db + 0.5 * LEVEL_KNEE_DB;
        slope * into_knee * into_knee / (2.0 * LEVEL_KNEE_DB)
    } else {
        slope * over_db
    }
}

/// The smallest of the last `window` pushes, by the monotonic-queue method
/// the [`Limiter`](crate::Limiter) uses, in storage of a fixed size.
struct SlidingMinimum {
    values: [f32; RING],
    stamps: [u64; RING],
    head: usize,
    len: usize,
}

impl SlidingMinimum {
    fn new() -> Self {
        Self {
            values: [1.0; RING],
            stamps: [0; RING],
            head: 0,
            len: 0,
        }
    }

    fn clear(&mut self) {
        self.head = 0;
        self.len = 0;
    }

    #[inline]
    fn push(&mut self, value: f32, now: u64, window: u64) -> f32 {
        while self.len > 0 && self.values[(self.head + self.len - 1) & MASK] >= value {
            self.len -= 1;
        }
        // At most `window` candidates are held, and a window is always
        // shorter than the ring.
        let tail = (self.head + self.len) & MASK;
        self.values[tail] = value;
        self.stamps[tail] = now;
        self.len += 1;
        while self.stamps[self.head] + window <= now {
            self.head = (self.head + 1) & MASK;
            self.len -= 1;
        }
        self.values[self.head]
    }
}

/// The average of the last `len` pushes.
struct MovingAverage {
    ring: [f32; RING],
    len: usize,
    position: usize,
    sum: f64,
}

impl MovingAverage {
    fn new() -> Self {
        Self {
            ring: [1.0; RING],
            len: 1,
            position: 0,
            sum: 1.0,
        }
    }

    fn refill(&mut self, len: usize, value: f32) {
        self.len = len.clamp(1, RING);
        self.ring[..self.len].fill(value);
        self.position = 0;
        self.sum = f64::from(value) * self.len as f64;
    }

    #[inline]
    fn push(&mut self, value: f32) -> f32 {
        self.sum += f64::from(value) - f64::from(self.ring[self.position]);
        self.ring[self.position] = value;
        self.position += 1;
        if self.position == self.len {
            self.position = 0;
        }
        (self.sum / self.len as f64) as f32
    }
}

/// A three-stage mastering limiter. See the module documentation.
///
/// The peak stage works the way the [`Limiter`](crate::Limiter) does: the
/// gain each peak needs is held for the look-ahead, recovers at the release
/// rate and is smoothed by two moving averages whose lengths add up to the
/// look-ahead, so the gain has arrived by the time the peak leaves the
/// delay. What it measures is different. With True peak on it reads the
/// waveform at four points per sample and holds each reading for the
/// sample either side of it.
///
/// The gain is applied to the samples, and a gain that moves changes the
/// waveform between them a little, so a moving gain can leave the true peak
/// a fraction of a dB over the ceiling. A steady signal is held exactly.
/// The samples themselves never exceed the ceiling.
///
/// The latency is fixed for a sample rate, see [`master_limiter_latency`].
/// Nothing is allocated, here or in `prepare`: every buffer is part of the
/// processor.
pub struct MasterLimiter {
    sample_rate: f32,
    params: MasterLimiterParams,
    /// Drive, level slope, saturation amount, tone.
    controls: Controls<4>,
    /// The ceiling as a linear level, without smoothing.
    ceiling: f32,

    level_detector: MeanSquare,
    /// Reduction of the level stage in dB, zero or more.
    level: Ballistics,

    color_dc: [DcBlocker; 2],

    detectors: [PeakInterpolator; 2],
    previous_peak: f32,
    /// Look-ahead in samples, without the detector's delay.
    lookahead: usize,
    delay: [[f32; RING]; 2],
    write: usize,
    minimum: SlidingMinimum,
    recovering: f32,
    release: f32,
    first_average: MovingAverage,
    second_average: MovingAverage,
    clock: u64,

    meter: GainReductionMeter,
    level_meter: GainReductionMeter,
    peak_meter: GainReductionMeter,
}

impl Default for MasterLimiter {
    fn default() -> Self {
        let mut limiter = Self {
            sample_rate: 48_000.0,
            params: MasterLimiterParams::default(),
            controls: Controls::new([1.0, 0.0, 0.0, 0.0]),
            ceiling: 1.0,
            level_detector: MeanSquare::new(LEVEL_WINDOW_MS, 48_000.0),
            level: Ballistics::new(0.0),
            color_dc: [DcBlocker::new(COLOR_DC_HZ, 48_000.0); 2],
            detectors: [PeakInterpolator::default(); 2],
            previous_peak: 0.0,
            lookahead: 1,
            delay: [[0.0; RING]; 2],
            write: 0,
            minimum: SlidingMinimum::new(),
            recovering: 1.0,
            release: 1.0,
            first_average: MovingAverage::new(),
            second_average: MovingAverage::new(),
            clock: 0,
            meter: GainReductionMeter::default(),
            level_meter: GainReductionMeter::default(),
            peak_meter: GainReductionMeter::default(),
        };
        limiter.configure(48_000.0);
        limiter
    }
}

impl MasterLimiter {
    /// Reads the reduction of the level stage and the peak stage added
    /// together, in dB. Take it before the limiter goes to the audio
    /// thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.meter.clone()
    }

    /// The meters of the level stage and of the peak stage, apart.
    pub fn stage_meters(&self) -> [GainReductionMeter; 2] {
        [self.level_meter.clone(), self.peak_meter.clone()]
    }

    fn configure(&mut self, sample_rate: f32) {
        self.sample_rate = rate(sample_rate);
        self.lookahead = master_limiter_latency(self.sample_rate) - PEAK_DELAY;
        self.level_detector = MeanSquare::new(LEVEL_WINDOW_MS, self.sample_rate);
        self.level
            .set_times(LEVEL_ATTACK_MS, LEVEL_RELEASE_MS, self.sample_rate);
        self.color_dc = [DcBlocker::new(COLOR_DC_HZ, self.sample_rate); 2];
        self.controls.prepare(self.sample_rate);
        self.reset();
    }

    fn apply(&mut self) {
        let params = self.params;
        self.ceiling = db_to_gain(params.ceiling_db);
        self.release = smoothing_coefficient(params.release_ms, self.sample_rate);
        self.controls.set([
            db_to_gain(params.gain_db),
            LEVEL_MAX_SLOPE * params.leveling,
            params.saturation,
            params.tone,
        ]);
    }
}

impl Effect for MasterLimiter {
    type Params = MasterLimiterParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.configure(sample_rate);
    }

    fn reset(&mut self) {
        self.controls.reset();
        self.level_detector.reset();
        self.level.snap(0.0);
        for blocker in &mut self.color_dc {
            blocker.reset();
        }
        for detector in &mut self.detectors {
            detector.reset();
        }
        self.previous_peak = 0.0;
        self.delay = [[0.0; RING]; 2];
        self.write = 0;
        self.minimum.clear();
        self.recovering = 1.0;
        self.clock = 0;
        // Two averages whose lengths add up to the look-ahead plus two,
        // which is what makes the smoothed gain arrive in time.
        let total = self.lookahead + 2;
        let first = total.div_ceil(2);
        self.first_average.refill(first, 1.0);
        self.second_average.refill(total - first, 1.0);
        self.apply();
    }

    fn set_params(&mut self, params: &MasterLimiterParams) {
        self.params = params.sanitized();
        self.apply();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let window = self.lookahead as u64 + 1;
        let delay = self.lookahead + PEAK_DELAY;
        let ceiling = self.ceiling;
        let true_peak = self.params.true_peak;
        let mut lowest = 1.0_f32;
        let mut deepest = 0.0_f32;
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let [drive, slope, saturation, tone] = self.controls.tick();
            let mut frame = [audio(*left) * drive, audio(*right) * drive];

            // Level: one slow gain from the average of both channels.
            let level_db = self.level_detector.tick_db(frame[0], frame[1]);
            let reduction = self
                .level
                .tick(level_reduction_db(level_db - LEVEL_THRESHOLD_DB, slope));
            deepest = deepest.max(reduction);
            let level_gain = db_to_gain_exp(-reduction);

            // Color: what the curve changes, without the offset its even
            // part adds, blended in.
            let tone = tone.clamp(0.0, 1.0);
            for (sample, blocker) in frame.iter_mut().zip(&mut self.color_dc) {
                let leveled = *sample * level_gain;
                let color = blocker.tick(super::saturate(leveled, 1.0, tone) - leveled);
                *sample = audio(leveled + saturation * color);
            }

            // Peak: the readings are of the frame PEAK_DELAY ago, which
            // leaves the delay `lookahead` frames from now.
            let (left_sample, left_peak) = self.detectors[0].tick(frame[0]);
            let (right_sample, right_peak) = self.detectors[1].tick(frame[1]);
            let covered = if true_peak {
                let peak = left_peak.max(right_peak);
                // A reading spans the gap to the next sample, so it binds
                // the gain of the sample at either end of it.
                let covered = peak.max(self.previous_peak);
                self.previous_peak = peak;
                covered
            } else {
                self.previous_peak = 0.0;
                left_sample.max(right_sample)
            };
            let needed = if covered > ceiling {
                ceiling / covered
            } else {
                1.0
            };
            let held = self.minimum.push(needed, self.clock, window);
            self.clock += 1;
            self.recovering = if held < self.recovering {
                held
            } else {
                self.recovering + (held - self.recovering) * self.release
            };
            let gain = self
                .second_average
                .push(self.first_average.push(self.recovering));
            lowest = lowest.min(gain);

            let at = self.write.wrapping_sub(delay) & MASK;
            let out = [self.delay[0][at], self.delay[1][at]];
            self.delay[0][self.write] = frame[0];
            self.delay[1][self.write] = frame[1];
            self.write = (self.write + 1) & MASK;

            // The clip acts on rounding, and on a ceiling lowered under
            // audio that is already inside the delay.
            *left = (out[0] * gain).clamp(-ceiling, ceiling);
            *right = (out[1] * gain).clamp(-ceiling, ceiling);
        }
        let peak_db = (-level_to_db(lowest)).max(0.0);
        self.level_meter.raise(deepest);
        self.peak_meter.raise(peak_db);
        self.meter.raise(deepest + peak_db);
    }

    fn latency_samples(&self) -> usize {
        self.lookahead + PEAK_DELAY
    }

    fn tail_samples(&self) -> usize {
        // The look-ahead, and the color stage's offset filter settling.
        self.latency_samples() + (self.sample_rate * 0.5) as usize
    }

    fn gap_samples(&self) -> usize {
        // While either stage lets go, a tail that is dying away can get
        // louder again.
        self.latency_samples()
            + ms_to_samples(
                LEVEL_RELEASE_MS.max(self.params.release_ms),
                self.sample_rate,
            ) as usize
    }
}
