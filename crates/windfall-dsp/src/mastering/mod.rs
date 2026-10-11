//! Stage Stack: soft harmonic shaping followed by a zero-delay safety clip.
//!
//! The submodules hold what a master is measured by ([`loudness`]) and the
//! larger limiter this one is the short form of ([`master_limiter`]).

pub mod chain;
pub mod loudness;
pub mod master_limiter;

pub use chain::{
    MASTERING_CEILING_RANGE, MASTERING_PEAK_TOLERANCE_DB, MASTERING_TARGET_RANGE,
    MASTERING_TOLERANCE_LU, Mastering, MasteringError, MasteringReport, MasteringSettings, master,
};
pub use loudness::{LOUDNESS_FLOOR, LoudnessMeter, LoudnessMetrics, measure};
pub use master_limiter::{MasterLimiter, MasterLimiterParams, master_limiter_latency};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::balance::{Controls, rate};
use crate::blocks::math::{db_to_gain, level_to_db, smoothing_coefficient};
use crate::effect::{Effect, GainReductionMeter};
use crate::param::{ParamSet, param_set};

/// Fixed recovery time of the safety stage, in milliseconds.
const RELEASE_MS: f32 = 3.0;

/// Settings for Windfall's [`StageStack`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct StageStackParams {
    /// Saturator input gain in dB, 0 to 36; default 0.
    pub drive_db: f32,
    /// Harmonic color, 0 (odd only) to 1 (added even harmonics); default 0.
    pub tone: f32,
    /// Wet sample ceiling in dBFS, -24 to 0; default 0.
    pub ceiling_db: f32,
    /// Dry/wet blend, 0 to 1; default 1. Dry audio bypasses the ceiling.
    pub mix: f32,
}

impl Default for StageStackParams {
    fn default() -> Self {
        Self {
            drive_db: 0.0,
            tone: 0.0,
            ceiling_db: 0.0,
            mix: 1.0,
        }
    }
}

param_set!(StageStackParams, "Stage Stack", {
    float [drive_db] "driveDb" "Drive" { Decibels, Linear, 0.0, 36.0, 0.0 }
    float [tone] "tone" "Odd/even tone" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [ceiling_db] "ceilingDb" "Ceiling" { Decibels, Linear, -24.0, 0.0, 0.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
});

/// Two series stages with per-sample control ramps and no audio delay.
///
/// The safety stage links stereo gain, attacks immediately and recovers over
/// 3 ms. It stores only the previous gain, with no look-ahead or audio buffers.
pub struct StageStack {
    controls: Controls<4>,
    ceiling_target: f32,
    safety_gain: f32,
    release: f32,
    meter: GainReductionMeter,
}

impl Default for StageStack {
    fn default() -> Self {
        Self {
            controls: Controls::new([1.0, 0.0, 1.0, 1.0]),
            ceiling_target: 1.0,
            safety_gain: 1.0,
            release: smoothing_coefficient(RELEASE_MS, 48_000.0),
            meter: GainReductionMeter::default(),
        }
    }
}

impl StageStack {
    /// Reads how far the safety stage turned the saturated signal down.
    /// Take it before the effect goes to the audio thread.
    pub fn meter(&self) -> GainReductionMeter {
        self.meter.clone()
    }
}

// Preserve every finite dry sample, including levels outside nominal full scale.
fn finite_input(input: f32) -> f32 {
    if input.is_finite() { input } else { 0.0 }
}

fn saturate(input: f32, drive: f32, tone: f32) -> f32 {
    // Double precision prevents overflow even for f32::MAX at maximum drive.
    // t is bounded, so the even polynomial is bounded as well and vanishes
    // both at zero and at the asymptotes. The odd curve has unity small-signal
    // slope at neutral drive; it never imposes a hard ceiling.
    let t = (f64::from(input) * f64::from(drive) * 0.5).tanh();
    let squared = t * t;
    (2.0 * t + f64::from(tone) * 0.5 * squared * (1.0 - squared)) as f32
}

fn blend(dry: f32, wet: f32, mix: f32) -> f32 {
    if mix == 0.0 {
        dry
    } else if mix == 1.0 {
        wet
    } else {
        // A convex blend stays finite even when the dry value is f32::MAX.
        (f64::from(dry) * (1.0 - f64::from(mix)) + f64::from(wet) * f64::from(mix)) as f32
    }
}

impl Effect for StageStack {
    type Params = StageStackParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        let sample_rate = rate(sample_rate);
        self.controls.prepare(sample_rate);
        self.release = smoothing_coefficient(RELEASE_MS, sample_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.controls.reset();
        self.safety_gain = 1.0;
    }

    fn set_params(&mut self, params: &Self::Params) {
        let params = params.sanitized();
        self.ceiling_target = db_to_gain(params.ceiling_db);
        self.controls.set([
            db_to_gain(params.drive_db),
            params.tone,
            self.ceiling_target,
            params.mix,
        ]);
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mut lowest = 1.0_f32;
        for (left, right) in left.iter_mut().zip(right) {
            let [drive, tone, ceiling, mix] = self.controls.tick();
            let tone = tone.clamp(0.0, 1.0);
            let mix = mix.clamp(0.0, 1.0);
            // Lowering a safety ceiling takes effect immediately. Raising it
            // uses the normal control ramp; release still retains past peaks.
            let ceiling = ceiling.min(self.ceiling_target);
            let dry_left = finite_input(*left);
            let dry_right = finite_input(*right);
            let wet_left = saturate(dry_left, drive, tone);
            let wet_right = saturate(dry_right, drive, tone);
            let peak = wet_left.abs().max(wet_right.abs());
            let needed = if peak > ceiling { ceiling / peak } else { 1.0 };
            self.safety_gain = if needed < self.safety_gain {
                needed
            } else {
                self.safety_gain + (needed - self.safety_gain) * self.release
            };
            lowest = lowest.min(self.safety_gain);
            let wet_left = (wet_left * self.safety_gain).clamp(-ceiling, ceiling);
            let wet_right = (wet_right * self.safety_gain).clamp(-ceiling, ceiling);
            *left = blend(dry_left, wet_left, mix);
            *right = blend(dry_right, wet_right, mix);
        }
        if lowest < 1.0 {
            self.meter.raise(-level_to_db(lowest));
        }
    }

    fn latency_samples(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests;
