//! A look-ahead limiter: nothing comes out louder than the ceiling.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::SMOOTHING_MS;
use crate::blocks::delay_line::DelayLine;
use crate::blocks::envelope::Ballistics;
use crate::blocks::math::{
    db_to_gain, db_to_gain_exp, level_to_db, ms_to_samples, smoothing_coefficient,
};
use crate::blocks::smooth::LinearRamp;
use crate::effect::{Effect, GainReductionMeter};
use crate::observe::{DynamicsObservation, GATE_FLOOR_DB};
use crate::param::{ParamSet, param_set};

/// Longest look-ahead, in ms. Buffers are sized for it.
const MAX_LOOKAHEAD_MS: f32 = 20.0;

/// Length of the crossfade between the old and new delay when the
/// look-ahead is changed while audio is running.
pub(crate) const LOOKAHEAD_FADE_MS: f32 = 5.0;

/// A gate threshold at or below this is the gate switched off.
pub const GATE_OFF_DB: f32 = -90.0;

/// How quickly a gate opens: the time to complete 63% of the way, in ms.
const GATE_OPEN_MS: f32 = 1.0;

/// How quickly the gate's level reading falls after a peak, in ms. Long
/// enough to bridge the zero crossings of a 20 Hz wave, so a steady low
/// note does not chatter.
const GATE_DETECTOR_MS: f32 = 25.0;

/// A ratio at or above this holds the level at the threshold.
const MAX_RATIO: f32 = 100.0;

/// What the limiter does to the signal before its ceiling.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum LimiterMode {
    /// Only the ceiling acts: peaks are held under it and nothing else
    /// changes.
    #[default]
    Limit,
    /// The signal is compressed above the threshold first, by the ratio,
    /// knee and attack, and then held under the ceiling.
    Compress,
}

/// Settings of the [`Limiter`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct LimiterParams {
    /// The level no output sample exceeds, in dB relative to full scale.
    /// -24 to 0, default -0.3.
    pub ceiling_db: f32,
    /// Gain applied before limiting, in dB. Turning it up drives the signal
    /// into the ceiling and makes it louder. -12 to 24, default 0.
    pub input_gain_db: f32,
    /// How quickly the gain recovers after a peak has passed: the time to
    /// complete 63% of the recovery, in ms. 1 to 1000, default 100. Short
    /// times are louder but can distort bass; long times are cleaner but
    /// can duck the signal audibly after a loud hit.
    pub release_ms: f32,
    /// How far ahead the limiter looks, in ms, which is also how long it
    /// delays the signal. 0.1 to 20, default 5. Longer look-ahead turns
    /// down more gently before a peak arrives and so distorts less.
    pub lookahead_ms: f32,
    /// Limit only, or compress before the ceiling. Default limit.
    pub mode: LimiterMode,
    /// In compress mode, the level above which the signal is turned down,
    /// in dB relative to full scale, measured after the input gain. -60 to
    /// 0, default -18.
    pub threshold_db: f32,
    /// In compress mode, how hard: for every `ratio` dB the input rises
    /// above the threshold the output rises 1 dB. 1 to 100, default 4.
    pub ratio: f32,
    /// In compress mode, the width in dB of the region around the
    /// threshold over which the ratio eases in. 0 to 24, default 6.
    pub knee_db: f32,
    /// In compress mode, how quickly the gain drops when the signal gets
    /// louder: the time to complete 63% of the change, in ms. 0.05 to 250,
    /// default 10. The recovery uses `release_ms`.
    pub attack_ms: f32,
    /// The level, in dB after the input gain, below which the gate shuts
    /// and the signal is silenced. -90 to 0. At -90, the default, there is
    /// no gate.
    pub gate_threshold_db: f32,
    /// How quickly the gate shuts once the signal has fallen under its
    /// threshold: the time to complete 63% of the way, in ms. 5 to 2500,
    /// default 200.
    pub gate_release_ms: f32,
}

impl Default for LimiterParams {
    fn default() -> Self {
        Self {
            ceiling_db: -0.3,
            input_gain_db: 0.0,
            release_ms: 100.0,
            lookahead_ms: 5.0,
            mode: LimiterMode::Limit,
            threshold_db: -18.0,
            ratio: 4.0,
            knee_db: 6.0,
            attack_ms: 10.0,
            gate_threshold_db: GATE_OFF_DB,
            gate_release_ms: 200.0,
        }
    }
}

impl LimiterParams {
    /// The latency a prepared limiter has with these settings at
    /// `sample_rate`: its look-ahead in samples. A host can work out delay
    /// compensation from this before the settings reach the limiter.
    pub fn latency_samples(&self, sample_rate: f32) -> usize {
        ms_to_samples(self.sanitized().lookahead_ms, sample_rate.max(1.0)) as usize
    }

    /// Whether these settings have a gate at all.
    pub fn gated(&self) -> bool {
        self.sanitized().gate_threshold_db > GATE_OFF_DB
    }

    /// The gain change in dB, zero or negative, that compress mode applies
    /// to a steady signal at `level_db` after the input gain, before the
    /// ceiling. Zero in limit mode. This is the curve a display draws.
    pub fn static_gain_db(&self, level_db: f32) -> f32 {
        let params = self.sanitized();
        if params.mode != LimiterMode::Compress {
            return 0.0;
        }
        -compression_db(
            level_db - params.threshold_db,
            compression_slope(params.ratio),
            params.knee_db,
        )
    }
}

/// The share of every dB over the threshold that is taken away: 0 at a
/// ratio of 1, 1 at the top ratio.
fn compression_slope(ratio: f32) -> f32 {
    if ratio >= MAX_RATIO {
        1.0
    } else {
        1.0 - 1.0 / ratio.max(1.0)
    }
}

/// Reduction in dB, zero or more, for a signal `over_db` above the
/// threshold. Inside the knee the slope eases in along a parabola.
#[inline]
fn compression_db(over_db: f32, slope: f32, knee_db: f32) -> f32 {
    if 2.0 * over_db <= -knee_db {
        0.0
    } else if 2.0 * over_db < knee_db {
        let into_knee = over_db + 0.5 * knee_db;
        slope * into_knee * into_knee / (2.0 * knee_db)
    } else {
        slope * over_db
    }
}

param_set!(LimiterParams, "Limiter", {
    float [ceiling_db] "ceilingDb" "Ceiling" { Decibels, Linear, -24.0, 0.0, -0.3 }
    float [input_gain_db] "inputGainDb" "Input gain" { Decibels, Linear, -12.0, 24.0, 0.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 1.0, 1000.0, 100.0 }
    float [lookahead_ms] "lookaheadMs" "Look-ahead" { Milliseconds, Logarithmic, 0.1, 20.0, 5.0 }
    choice [mode] "mode" "Mode" {
        LimiterMode, Limit, [Limit "limit" "Limit", Compress "compress" "Compress"]
    }
    float [threshold_db] "thresholdDb" "Threshold" { Decibels, Linear, -60.0, 0.0, -18.0 }
    float [ratio] "ratio" "Ratio" { Ratio, Logarithmic, 1.0, 100.0, 4.0 }
    float [knee_db] "kneeDb" "Knee" { Decibels, Linear, 0.0, 24.0, 6.0 }
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Logarithmic, 0.05, 250.0, 10.0 }
    float [gate_threshold_db] "gateThresholdDb" "Gate" { Decibels, Linear, -90.0, 0.0, -90.0 }
    float [gate_release_ms] "gateReleaseMs" "Gate release" { Milliseconds, Logarithmic, 5.0, 2500.0, 200.0 }
});

/// The smallest value of the last `window` pushes, in constant time per
/// push on average.
///
/// This is the monotonic-queue method (Daniel Lemire, "Streaming
/// maximum-minimum filter using no more than three comparisons per
/// element", 2006): candidates that can never be the minimum again are
/// dropped as soon as a smaller value arrives behind them.
struct SlidingMinimum {
    values: Box<[f32]>,
    stamps: Box<[u64]>,
    mask: usize,
    head: usize,
    len: usize,
}

impl SlidingMinimum {
    fn new(max_window: usize) -> Self {
        let capacity = (max_window + 1).next_power_of_two();
        Self {
            values: vec![1.0; capacity].into_boxed_slice(),
            stamps: vec![0; capacity].into_boxed_slice(),
            mask: capacity - 1,
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
        while self.len > 0 && self.values[(self.head + self.len - 1) & self.mask] >= value {
            self.len -= 1;
        }
        // At most `window` candidates are ever held, and the ring has room
        // for one more than the longest window.
        let tail = (self.head + self.len) & self.mask;
        self.values[tail] = value;
        self.stamps[tail] = now;
        self.len += 1;
        while self.stamps[self.head] + window <= now {
            self.head = (self.head + 1) & self.mask;
            self.len -= 1;
        }
        self.values[self.head]
    }
}

/// The average of the last `len` pushes.
struct MovingAverage {
    ring: Box<[f32]>,
    len: usize,
    position: usize,
    /// Double precision, so that adding and removing millions of values
    /// leaves no drift.
    sum: f64,
}

impl MovingAverage {
    fn new(max_len: usize) -> Self {
        Self {
            ring: vec![1.0; max_len.max(1)].into_boxed_slice(),
            len: 1,
            position: 0,
            sum: 1.0,
        }
    }

    /// Sets the window length and fills it with `value`.
    fn refill(&mut self, len: usize, value: f32) {
        self.len = len.clamp(1, self.ring.len());
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

/// A brickwall look-ahead limiter.
///
/// The signal is delayed by the look-ahead time. Meanwhile the limiter
/// works out, for every incoming sample, the gain that would bring it under
/// the ceiling; holds the lowest such gain for the length of the look-ahead
/// (a sliding minimum); lets it recover at the release rate; and smooths
/// the result with two moving averages whose lengths add up to the
/// look-ahead. By the time a peak leaves the delay, the smoothed gain has
/// arrived at or below what that peak needs. That is true by construction
/// for any input, so the limiter never has to clip, yet the gain moves in
/// smooth S-shaped curves instead of steps. Both channels always get the
/// same gain.
///
/// On steady material the held minimum stops the gain from wobbling
/// between one cycle's peak and the next, so the limiter does not pump or
/// distort anything whose period is shorter than twice the look-ahead.
///
/// The latency is the look-ahead, reported by
/// [`latency_samples`](Effect::latency_samples). Changing the look-ahead
/// while audio runs crossfades between the old delay and the new one.
/// During that crossfade, and while the ceiling is being lowered, the
/// guarantee is kept by a plain clip at the ceiling.
///
/// Ahead of all that sit two stages that only ever turn the signal down,
/// so they take nothing from the guarantee. A gate silences the signal
/// while its level is under the gate threshold, opening fast and shutting
/// at its release time. In compress mode a feed-forward compressor then
/// turns down what is over its threshold, by its ratio, knee and attack,
/// and lets go at the limiter's release time. Both measure the peak of the
/// louder channel after the input gain, and both channels get the same
/// gain. With the gate off and in limit mode both stages multiply by
/// exactly one.
pub struct Limiter {
    sample_rate: f32,
    params: LimiterParams,
    /// Input gain divided by the ceiling: the limiter itself works against
    /// a ceiling of 1.
    drive: LinearRamp,
    ceiling: LinearRamp,
    /// The ceiling as a linear level, without smoothing.
    ceiling_now: f32,
    /// Look-ahead in samples.
    lookahead: usize,
    delay_left: DelayLine,
    delay_right: DelayLine,
    minimum: SlidingMinimum,
    /// The held minimum after the release has been applied.
    recovering: f32,
    release: f32,
    first_average: MovingAverage,
    second_average: MovingAverage,
    /// The gain applied to the last sample.
    gain: f32,
    /// The delay being faded out after a look-ahead change, and how many
    /// samples of that fade are left.
    old_lookahead: usize,
    fade_left: u32,
    fade_len: u32,
    clock: u64,
    meter: GainReductionMeter,
    fresh: bool,
    /// The input gain as a linear factor, for the gate's and the
    /// compressor's level measurement.
    input_gain: f32,
    /// The level the gate goes by: it follows peaks at once and falls
    /// slowly, as a linear level.
    gate_level: Ballistics,
    /// The gate's opening, 0 (shut) to 1 (open).
    gate: Ballistics,
    /// Compression in dB, zero or more. Rising is the attack.
    compression: Ballistics,
    /// What the last block measured, and how many blocks there have been.
    observed: Option<DynamicsObservation>,
    blocks: u64,
}

impl Default for Limiter {
    fn default() -> Self {
        let mut limiter = Self {
            sample_rate: 48_000.0,
            params: LimiterParams::default(),
            drive: LinearRamp::new(1.0),
            ceiling: LinearRamp::new(1.0),
            ceiling_now: 1.0,
            // Zero is never a real look-ahead, so the first `apply` sets
            // the averages up.
            lookahead: 0,
            delay_left: DelayLine::default(),
            delay_right: DelayLine::default(),
            minimum: SlidingMinimum::new(2),
            recovering: 1.0,
            release: 1.0,
            first_average: MovingAverage::new(3),
            second_average: MovingAverage::new(3),

            gain: 1.0,
            old_lookahead: 1,
            fade_left: 0,
            fade_len: 1,
            clock: 0,
            meter: GainReductionMeter::default(),
            fresh: true,
            input_gain: 1.0,
            gate_level: Ballistics::new(0.0),
            gate: Ballistics::new(1.0),
            compression: Ballistics::new(0.0),
            observed: None,
            blocks: 0,
        };
        limiter.apply();
        limiter
    }
}

impl Limiter {
    /// The longest latency any look-ahead setting gives at `sample_rate`.
    pub fn max_latency_samples(sample_rate: f32) -> usize {
        ms_to_samples(MAX_LOOKAHEAD_MS, sample_rate) as usize
    }

    /// A handle for reading the gain reduction from another thread. Take it
    /// before the limiter goes to the audio thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.meter.clone()
    }

    /// What the last processed block measured. `None` until a block has
    /// been processed since the limiter was prepared or reset. Reading it
    /// changes nothing, the meter included.
    pub fn observation(&self) -> Option<DynamicsObservation> {
        self.observed
    }

    fn apply(&mut self) {
        let params = self.params;
        let samples = if self.fresh {
            0
        } else {
            ms_to_samples(SMOOTHING_MS, self.sample_rate)
        };
        self.ceiling_now = db_to_gain(params.ceiling_db);
        self.ceiling.set_target(self.ceiling_now, samples);
        self.drive.set_target(
            db_to_gain(params.input_gain_db - params.ceiling_db),
            samples,
        );
        self.release = smoothing_coefficient(params.release_ms, self.sample_rate);
        self.input_gain = db_to_gain(params.input_gain_db);
        self.gate_level
            .set_times(0.0, GATE_DETECTOR_MS, self.sample_rate);
        self.gate
            .set_times(GATE_OPEN_MS, params.gate_release_ms, self.sample_rate);
        self.compression
            .set_times(params.attack_ms, params.release_ms, self.sample_rate);

        let lookahead = params
            .latency_samples(self.sample_rate)
            .min(self.delay_left.max_delay())
            .max(1);
        if lookahead != self.lookahead {
            if self.fresh {
                self.fade_left = 0;
            } else if self.fade_left == 0 {
                self.old_lookahead = self.lookahead;
                self.fade_len = ms_to_samples(LOOKAHEAD_FADE_MS, self.sample_rate);
                self.fade_left = self.fade_len;
            }
            self.lookahead = lookahead;
            self.resize_averages();
        }
    }

    /// Sets the two moving averages to lengths that add up to the
    /// look-ahead plus two, which is what makes the smoothed gain arrive in
    /// time, and starts them from the gain in effect now.
    fn resize_averages(&mut self) {
        let total = self.lookahead + 2;
        let first = total.div_ceil(2);
        let start = self.gain.min(self.recovering);
        self.first_average.refill(first, start);
        self.second_average.refill(total - first, start);
    }

    /// The gain of the gate and of the compressor for one frame whose
    /// peak, before the input gain, is `raw_peak`. Never more than 1, and
    /// exactly 1 with the gate off in limit mode.
    #[inline]
    fn shape(&mut self, raw_peak: f32, gated: bool, compress: bool, block: &mut Block) -> f32 {
        let level = raw_peak * self.input_gain;
        if level.is_finite() {
            block.input_peak = block.input_peak.max(level);
        }
        let mut gain = 1.0_f32;
        if gated {
            let held = self
                .gate_level
                .tick(if level.is_finite() { level } else { 0.0 });
            let open = level_to_db(held) >= self.params.gate_threshold_db;
            let opening = self.gate.tick(if open { 1.0 } else { 0.0 }).clamp(0.0, 1.0);
            block.gate = block.gate.min(opening);
            gain = opening;
        }
        if compress {
            let wanted = compression_db(
                level_to_db(level) - self.params.threshold_db,
                compression_slope(self.params.ratio),
                self.params.knee_db,
            );
            let reduction = self.compression.tick(wanted).max(0.0);
            block.compression_db = block.compression_db.max(reduction);
            gain *= db_to_gain_exp(-reduction).min(1.0);
        }
        gain
    }

    /// The limiter proper: everything but the final safety clip. Returns
    /// what the block measured, the lowest limiting gain included.
    fn limit(&mut self, left: &mut [f32], right: &mut [f32]) -> Block {
        let window = self.lookahead as u64 + 1;
        let mut lowest = 1.0_f32;
        let mut block = Block::default();
        let gated = self.params.gate_threshold_db > GATE_OFF_DB;
        let compress = self.params.mode == LimiterMode::Compress;
        if !gated {
            self.gate.snap(1.0);
            self.gate_level.snap(0.0);
        }
        if !compress {
            self.compression.snap(0.0);
        }
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let drive = self.drive.tick();
            let shape = self.shape(left.abs().max(right.abs()), gated, compress, &mut block);
            // With nothing to shape this is the driven sample untouched:
            // multiplying by one changes no bit of it.
            let (in_left, in_right) = (*left * drive * shape, *right * drive * shape);
            let peak = in_left.abs().max(in_right.abs());
            let needed = if peak > 1.0 { 1.0 / peak } else { 1.0 };

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
            self.gain = gain;
            lowest = lowest.min(gain);

            let mut out_left = self.delay_left.tap(self.lookahead);
            let mut out_right = self.delay_right.tap(self.lookahead);
            if self.fade_left > 0 {
                let old = self.fade_left as f32 / self.fade_len as f32;
                out_left += (self.delay_left.tap(self.old_lookahead) - out_left) * old;
                out_right += (self.delay_right.tap(self.old_lookahead) - out_right) * old;
                self.fade_left -= 1;
            }
            self.delay_left.push(in_left);
            self.delay_right.push(in_right);

            let level = gain * self.ceiling.tick();
            *left = out_left * level;
            *right = out_right * level;
        }
        block.lowest = lowest;
        block
    }
}

/// What one call of [`Limiter::limit`] measured.
#[derive(Clone, Copy)]
struct Block {
    /// The lowest limiting gain applied.
    lowest: f32,
    /// The loudest input after the input gain, as a linear level.
    input_peak: f32,
    /// The furthest the gate shut, 1 (open) to 0.
    gate: f32,
    /// The deepest compression, in dB.
    compression_db: f32,
}

impl Default for Block {
    fn default() -> Self {
        Self {
            lowest: 1.0,
            input_peak: 0.0,
            gate: 1.0,
            compression_db: 0.0,
        }
    }
}

impl Effect for Limiter {
    type Params = LimiterParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = sample_rate.max(1.0);
        let longest = Self::max_latency_samples(self.sample_rate);
        self.delay_left = DelayLine::new(longest);
        self.delay_right = DelayLine::new(longest);
        self.minimum = SlidingMinimum::new(longest + 1);
        self.first_average = MovingAverage::new(longest + 2);
        self.second_average = MovingAverage::new(longest + 2);
        // Force the look-ahead to be worked out again for the new rate.
        self.lookahead = 0;
        self.reset();
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.delay_left.clear();
        self.delay_right.clear();
        self.minimum.clear();
        self.recovering = 1.0;
        self.gain = 1.0;
        self.fade_left = 0;
        self.clock = 0;
        self.gate.snap(1.0);
        self.gate_level.snap(0.0);
        self.compression.snap(0.0);
        self.observed = None;
        self.apply();
        self.resize_averages();
    }

    fn set_params(&mut self, params: &LimiterParams) {
        self.params = params.sanitized();
        self.apply();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        let block = self.limit(left, right);
        // The limiter already keeps every sample under the ceiling. This
        // clip only acts on rounding in the last digit, and on the two
        // cases the limiter cannot foresee: the ceiling being lowered and
        // the look-ahead being changed under audio that is already inside
        // the delay.
        let ceiling = self.ceiling_now;
        let mut output_peak = 0.0_f32;
        for sample in left.iter_mut().chain(right.iter_mut()) {
            *sample = sample.clamp(-ceiling, ceiling);
            output_peak = output_peak.max(sample.abs());
        }
        // The meter and the observation count what was turned down on
        // purpose: compression and limiting. A shut gate is told apart.
        let limiting_db = -level_to_db(block.lowest);
        let reduction_db = if block.compression_db > 0.0 {
            limiting_db.max(0.0) + block.compression_db
        } else {
            limiting_db
        };
        self.meter.raise(reduction_db);
        self.blocks = self.blocks.wrapping_add(1);
        self.observed = Some(DynamicsObservation {
            serial: self.blocks,
            frames: left.len().min(u32::MAX as usize) as u32,
            input_peak: block.input_peak,
            output_peak,
            reduction_db: reduction_db.max(0.0),
            gate_db: (-level_to_db(block.gate)).clamp(0.0, GATE_FLOOR_DB),
        });
    }

    fn latency_samples(&self) -> usize {
        self.lookahead
    }

    fn tail_samples(&self) -> usize {
        self.lookahead
    }

    fn gap_samples(&self) -> usize {
        // While the gain comes back up, a tail that is dying away can get
        // louder again.
        self.lookahead + ms_to_samples(self.params.release_ms, self.sample_rate) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::noise::Rng;

    #[test]
    fn sliding_minimum_matches_a_brute_force_search() {
        let mut rng = Rng::new(11);
        for window in [1_usize, 2, 7, 64] {
            let mut minimum = SlidingMinimum::new(64);
            let mut history = Vec::new();
            for now in 0..2_000_u64 {
                let value = rng.unipolar();
                history.push(value);
                let expected = history[history.len().saturating_sub(window)..]
                    .iter()
                    .fold(f32::INFINITY, |least, value| least.min(*value));
                assert_eq!(minimum.push(value, now, window as u64), expected);
            }
        }
    }

    #[test]
    fn sliding_minimum_survives_a_falling_then_rising_run() {
        let mut minimum = SlidingMinimum::new(8);
        let mut now = 0;
        for step in 0..100 {
            let value = 1.0 - step as f32 * 0.01;
            assert_eq!(minimum.push(value, now, 9), value);
            now += 1;
        }
        // A rising run keeps every value as a candidate until it expires.
        for step in 0..100 {
            let value = step as f32 * 0.01;
            let result = minimum.push(value, now, 9);
            assert!(result <= value);
            now += 1;
        }
    }

    #[test]
    fn moving_average_is_exact_and_does_not_drift() {
        let mut average = MovingAverage::new(16);
        average.refill(4, 1.0);
        assert_eq!(average.push(1.0), 1.0);
        assert!((average.push(0.0) - 0.75).abs() < 1e-7);
        assert!((average.push(0.0) - 0.5).abs() < 1e-7);
        let mut rng = Rng::new(3);
        for _ in 0..1_000_000 {
            average.push(rng.unipolar() * 1.0e-3 + rng.unipolar());
        }
        for _ in 0..4 {
            average.push(0.25);
        }
        assert!((average.push(0.25) - 0.25).abs() < 1e-7);
    }

    /// Signals chosen to catch a limiter out: full-scale steps, single
    /// samples far over the ceiling, bursts that start mid-release, slow
    /// swells and dense noise.
    fn adversarial(kind: usize, length: usize, rng: &mut Rng) -> Vec<f32> {
        (0..length)
            .map(|n| match kind {
                0 => {
                    if (n / 97) % 2 == 0 {
                        8.0
                    } else {
                        -8.0
                    }
                }
                1 => {
                    if n % 251 == 0 {
                        100.0
                    } else {
                        0.01
                    }
                }
                2 => rng.bipolar() * 16.0,
                3 => {
                    let burst = (n / 600) % 3 == 0;
                    (n as f32 * 0.9).sin() * if burst { 12.0 } else { 0.2 }
                }
                4 => (n as f32 * 0.002).sin() * (n as f32 * 0.0001).min(30.0),
                5 => {
                    if rng.unipolar() < 0.01 {
                        rng.bipolar() * 1.0e3
                    } else {
                        rng.bipolar() * 0.5
                    }
                }
                _ => 4.0,
            })
            .collect()
    }

    #[test]
    fn the_limiter_alone_keeps_every_signal_under_the_ceiling() {
        // `limit` is the process without the final clip, so this shows the
        // look-ahead does the work and the clip is only a backstop.
        let mut rng = Rng::new(21);
        for rate in [44_100.0, 96_000.0] {
            for lookahead_ms in [0.1, 1.0, 5.0, 20.0] {
                for release_ms in [1.0, 100.0, 1000.0] {
                    let mut limiter = Limiter::default();
                    limiter.prepare(rate, 512);
                    limiter.set_params(&LimiterParams {
                        ceiling_db: -6.0,
                        input_gain_db: 12.0,
                        release_ms,
                        lookahead_ms,
                        ..LimiterParams::default()
                    });
                    let ceiling = db_to_gain(-6.0);
                    for kind in 0..7 {
                        let mut left = adversarial(kind, 6_000, &mut rng);
                        let mut right = adversarial((kind + 3) % 7, 6_000, &mut rng);
                        for (left, right) in left.chunks_mut(61).zip(right.chunks_mut(61)) {
                            limiter.limit(left, right);
                        }
                        let peak = left
                            .iter()
                            .chain(right.iter())
                            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
                        assert!(
                            peak <= ceiling * 1.000_01,
                            "kind {kind}, look-ahead {lookahead_ms} ms, release {release_ms} ms \
                             at {rate}: {peak}"
                        );
                    }
                }
            }
        }
    }
}
