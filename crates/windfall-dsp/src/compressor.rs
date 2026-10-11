//! A compressor: turns loud passages down so the whole signal can be turned
//! up.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::SMOOTHING_MS;
use crate::blocks::envelope::{Ballistics, MeanSquare, peak_db};
use crate::blocks::math::{db_to_gain_exp, ms_to_samples};
use crate::blocks::smooth::LinearRamp;
use crate::effect::{Effect, GainReductionMeter};
use crate::param::{ParamSet, param_set};

/// A ratio at or above this is treated as infinity to one: nothing gets
/// louder than the threshold.
pub const COMPRESSOR_MAX_RATIO: f32 = 100.0;

/// Averaging time of the RMS detector.
const RMS_WINDOW_MS: f32 = 10.0;

/// How the compressor measures loudness.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DetectorMode {
    /// Reacts to the instantaneous peak of the louder channel. Fast and
    /// precise; the threshold is a peak level.
    #[default]
    Peak,
    /// Reacts to the average power over about 10 ms, which is closer to how
    /// loud something sounds. The threshold is an RMS level, so a sine
    /// reads 3 dB lower than in peak mode.
    Rms,
}

/// Settings of the [`Compressor`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct CompressorParams {
    /// Use the track's detector-only key bus; silence when no key arrives.
    #[serde(default)]
    #[ts(as = "Option<bool>", optional)]
    pub sidechain: bool,
    /// Level above which the signal is turned down, in dB relative to full
    /// scale. -60 to 0, default -18.
    pub threshold_db: f32,
    /// How hard it is turned down: for every `ratio` dB the input rises
    /// above the threshold, the output rises 1 dB. 1 (no compression) to
    /// 100, default 4. At 100 the output never rises above the threshold.
    pub ratio: f32,
    /// How quickly the gain drops when the signal gets louder: the time to
    /// complete 63% of the change, in ms. 0.05 to 250, default 10.
    pub attack_ms: f32,
    /// How quickly the gain recovers when the signal gets quieter: the time
    /// to complete 63% of the change, in ms. 5 to 2500, default 120.
    pub release_ms: f32,
    /// Width in dB of the region around the threshold over which the ratio
    /// eases in. 0 (a hard corner) to 24, default 6.
    pub knee_db: f32,
    /// Gain applied after compression, in dB. -24 to 24, default 0.
    pub makeup_db: f32,
    /// Adds makeup gain that follows the threshold, ratio and knee: half of
    /// what a full-scale signal is turned down by. Default off.
    pub auto_makeup: bool,
    /// How loudness is measured. Default peak.
    pub detector: DetectorMode,
    /// Balance between the untouched input (0) and the compressed signal
    /// (1). Values in between give parallel compression. Default 1.
    pub mix: f32,
}

impl Default for CompressorParams {
    fn default() -> Self {
        Self {
            sidechain: false,
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 120.0,
            knee_db: 6.0,
            makeup_db: 0.0,
            auto_makeup: false,
            detector: DetectorMode::Peak,
            mix: 1.0,
        }
    }
}

param_set!(CompressorParams, "Compressor", {
    float [threshold_db] "thresholdDb" "Threshold" { Decibels, Linear, -60.0, 0.0, -18.0 }
    float [ratio] "ratio" "Ratio" { Ratio, Logarithmic, 1.0, 100.0, 4.0 }
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Logarithmic, 0.05, 250.0, 10.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 5.0, 2500.0, 120.0 }
    float [knee_db] "kneeDb" "Knee" { Decibels, Linear, 0.0, 24.0, 6.0 }
    float [makeup_db] "makeupDb" "Makeup" { Decibels, Linear, -24.0, 24.0, 0.0 }
    toggle [auto_makeup] "autoMakeup" "Auto makeup" { false }
    choice [detector] "detector" "Detector" {
        DetectorMode, Peak, [Peak "peak" "Peak", Rms "rms" "RMS"]
    }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [sidechain] "sidechain" "External sidechain" { false }
});

/// The share of every dB over the threshold that is taken away: 0 at a
/// ratio of 1, 1 at infinity.
fn reduction_slope(ratio: f32) -> f32 {
    if ratio >= COMPRESSOR_MAX_RATIO {
        1.0
    } else {
        1.0 - 1.0 / ratio.max(1.0)
    }
}

/// Reduction in dB, zero or more, for a signal `over_db` above the
/// threshold. Inside the knee the slope eases in along a parabola.
#[inline]
fn reduction_db(over_db: f32, slope: f32, knee_db: f32) -> f32 {
    if 2.0 * over_db <= -knee_db {
        0.0
    } else if 2.0 * over_db < knee_db {
        let into_knee = over_db + 0.5 * knee_db;
        slope * into_knee * into_knee / (2.0 * knee_db)
    } else {
        slope * over_db
    }
}

impl CompressorParams {
    /// The gain change in dB, zero or negative, that these settings apply
    /// to a steady signal at `level_db`, before makeup gain. This is the
    /// curve a compressor's display draws.
    pub fn static_gain_db(&self, level_db: f32) -> f32 {
        let params = self.sanitized();
        -reduction_db(
            level_db - params.threshold_db,
            reduction_slope(params.ratio),
            params.knee_db,
        )
    }

    /// The makeup gain in dB in effect, including the automatic part.
    pub fn total_makeup_db(&self) -> f32 {
        let params = self.sanitized();
        if params.auto_makeup {
            params.makeup_db - 0.5 * params.static_gain_db(0.0)
        } else {
            params.makeup_db
        }
    }
}

/// A feed-forward, stereo-linked compressor.
///
/// Both channels are measured together and get the same gain, so the stereo
/// image does not shift. Level is measured in dB, run through the
/// threshold, ratio and knee, and the resulting gain reduction is smoothed
/// in dB by the attack and release (the layout Giannoulis, Massberg and
/// Reiss recommend in "Digital Dynamic Range Compressor Design", JAES
/// 2012). It adds no latency.
pub struct Compressor {
    sample_rate: f32,
    params: CompressorParams,
    threshold: LinearRamp,
    slope: LinearRamp,
    knee: LinearRamp,
    makeup: LinearRamp,
    mix: LinearRamp,
    rms: MeanSquare,
    /// Gain reduction in dB, zero or more. Rising is the attack.
    reduction: Ballistics,
    meter: GainReductionMeter,
    /// True until the first block after `prepare` or `reset`: parameter
    /// changes take effect at once instead of gliding.
    fresh: bool,
}

impl Default for Compressor {
    fn default() -> Self {
        let mut compressor = Self {
            sample_rate: 48_000.0,
            params: CompressorParams::default(),
            threshold: LinearRamp::new(0.0),
            slope: LinearRamp::new(0.0),
            knee: LinearRamp::new(0.0),
            makeup: LinearRamp::new(0.0),
            mix: LinearRamp::new(1.0),
            rms: MeanSquare::new(RMS_WINDOW_MS, 48_000.0),
            reduction: Ballistics::new(0.0),
            meter: GainReductionMeter::default(),
            fresh: true,
        };
        compressor.apply();
        compressor
    }
}

impl Compressor {
    /// A handle for reading the gain reduction from another thread. Take it
    /// before the compressor goes to the audio thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.meter.clone()
    }

    fn apply(&mut self) {
        let params = &self.params;
        let samples = if self.fresh {
            0
        } else {
            ms_to_samples(SMOOTHING_MS, self.sample_rate)
        };
        self.threshold.set_target(params.threshold_db, samples);
        self.slope
            .set_target(reduction_slope(params.ratio), samples);
        self.knee.set_target(params.knee_db, samples);
        self.makeup.set_target(params.total_makeup_db(), samples);
        self.mix.set_target(params.mix, samples);
        self.reduction
            .set_times(params.attack_ms, params.release_ms, self.sample_rate);
    }
}

impl Compressor {
    fn process_detector(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        self.fresh = false;
        let peak_mode = self.params.detector == DetectorMode::Peak;
        let mut deepest = 0.0_f32;
        for (index, (left, right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let input = if self.params.sidechain {
                key.and_then(|key| key.get(index))
                    .copied()
                    .unwrap_or([0.0; 2])
            } else {
                [*left, *right]
            };
            // The RMS average runs in both modes so that switching to it
            // does not start from a stale value.
            let rms = self.rms.tick_db(input[0], input[1]);
            let level = if peak_mode {
                peak_db(input[0], input[1])
            } else {
                rms
            };
            let wanted = reduction_db(
                level - self.threshold.tick(),
                self.slope.tick(),
                self.knee.tick(),
            );
            let reduction = self.reduction.tick(wanted);
            deepest = deepest.max(reduction);
            let gain = db_to_gain_exp(self.makeup.tick() - reduction);
            let mix = self.mix.tick();
            *left += (*left * gain - *left) * mix;
            *right += (*right * gain - *right) * mix;
        }
        self.meter.raise(deepest);
    }
}

impl Effect for Compressor {
    type Params = CompressorParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = sample_rate.max(1.0);
        self.rms = MeanSquare::new(RMS_WINDOW_MS, self.sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.fresh = true;
        self.apply();
        self.rms.reset();
        self.reduction.snap(0.0);
    }

    fn set_params(&mut self, params: &CompressorParams) {
        self.params = params.sanitized();
        self.apply();
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_detector(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        self.process_detector(left, right, key);
    }

    fn gap_samples(&self) -> usize {
        // While the compressor lets go, a tail that is dying away can get
        // louder again.
        ms_to_samples(self.params.release_ms, self.sample_rate) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_joins_the_two_straight_parts_smoothly() {
        let slope = reduction_slope(4.0);
        let knee = 12.0;
        assert_eq!(reduction_db(-6.0, slope, knee), 0.0);
        assert!((reduction_db(6.0, slope, knee) - slope * 6.0).abs() < 1e-5);
        // Halfway through the knee a quarter of the full slope has built up.
        assert!((reduction_db(0.0, slope, knee) - slope * 1.5).abs() < 1e-5);
        let mut previous = 0.0;
        for step in -100..=100 {
            let reduction = reduction_db(step as f32 * 0.1, slope, knee);
            assert!(reduction >= previous - 1e-6);
            assert!(reduction - previous < slope * 0.1 + 1e-4);
            previous = reduction;
        }
    }

    #[test]
    fn a_hard_knee_is_a_corner() {
        let slope = reduction_slope(2.0);
        assert_eq!(reduction_db(-0.001, slope, 0.0), 0.0);
        assert!((reduction_db(10.0, slope, 0.0) - 5.0).abs() < 1e-6);
    }

    #[test]
    fn the_top_ratio_is_a_limiter() {
        assert_eq!(reduction_slope(COMPRESSOR_MAX_RATIO), 1.0);
        assert_eq!(reduction_slope(1.0), 0.0);
        assert!((reduction_slope(4.0) - 0.75).abs() < 1e-6);
    }
}
