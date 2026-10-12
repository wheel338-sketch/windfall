//! Integrated post-slot track processing, independent of the effect slot count.
use crate::blocks::smooth::LinearRamp;
use crate::param::{ParamSet, param_set};
use crate::{Effect, EqBand, EqParams, ParametricEq};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct TrackParams {
    pub eq_enabled: bool,
    pub low: EqBand,
    pub mid: EqBand,
    pub high: EqBand,
    pub invert_left: bool,
    pub invert_right: bool,
    pub swap: bool,
    /// -1 doubles side, 0 preserves stereo, +1 sums to mono.
    pub separation: f32,
}
impl Default for TrackParams {
    fn default() -> Self {
        let shelf = |frequency_hz| EqBand {
            frequency_hz,
            q: std::f32::consts::FRAC_1_SQRT_2,
            ..EqBand::default()
        };
        Self {
            eq_enabled: true,
            low: shelf(100.0),
            mid: EqBand::default(),
            high: shelf(8_000.0),
            invert_left: false,
            invert_right: false,
            swap: false,
            separation: 0.0,
        }
    }
}
impl TrackParams {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

// Indices are persisted by automation: append new controls, never reorder.
param_set!(TrackParams, "Track EQ and stereo", {
    toggle [eq_enabled] "eqEnabled" "Track EQ" { true }
    toggle [low.enabled] "low.enabled" "Low shelf" { true }
    float [low.frequency_hz] "low.frequencyHz" "Low frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 100.0 }
    float [low.gain_db] "low.gainDb" "Low gain" { Decibels, Linear, -24.0, 24.0, 0.0 }
    float [low.q] "low.q" "Low Q" { None, Logarithmic, 0.1, 2.0, std::f32::consts::FRAC_1_SQRT_2 }
    toggle [mid.enabled] "mid.enabled" "Mid bell" { true }
    float [mid.frequency_hz] "mid.frequencyHz" "Mid frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [mid.gain_db] "mid.gainDb" "Mid gain" { Decibels, Linear, -24.0, 24.0, 0.0 }
    float [mid.q] "mid.q" "Mid Q" { None, Logarithmic, 0.1, 18.0, 1.0 }
    toggle [high.enabled] "high.enabled" "High shelf" { true }
    float [high.frequency_hz] "high.frequencyHz" "High frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 8_000.0 }
    float [high.gain_db] "high.gainDb" "High gain" { Decibels, Linear, -24.0, 24.0, 0.0 }
    float [high.q] "high.q" "High Q" { None, Logarithmic, 0.1, 2.0, std::f32::consts::FRAC_1_SQRT_2 }
    toggle [invert_left] "invertLeft" "Invert left polarity" { false }
    toggle [invert_right] "invertRight" "Invert right polarity" { false }
    toggle [swap] "swap" "Swap left and right" { false }
    float [separation] "separation" "Stereo separation" { None, Linear, -1.0, 1.0, 0.0 }
});

pub struct TrackProcessor {
    pub params: TrackParams,
    eq: ParametricEq,
    matrix: [LinearRamp; 4],
    fade: u32,
}
impl TrackProcessor {
    /// Clear filter history while retaining all prepared buffers.
    pub fn reset(&mut self) {
        self.eq.reset();
        self.apply(true);
    }
    pub fn new(rate: u32, block: usize, params: TrackParams) -> Self {
        let mut unit = Self {
            params: params.sanitized(),
            eq: ParametricEq::default(),
            matrix: [LinearRamp::new(0.0); 4],
            fade: (rate.max(1) as f64 * 0.005).round().max(1.0) as u32,
        };
        unit.eq.prepare(rate.max(1) as f32, block);
        unit.apply(true);
        unit
    }
    fn apply(&mut self, snap: bool) {
        let p = self.params;
        let mut low = p.low;
        low.enabled &= p.eq_enabled;
        let mut mid = p.mid;
        mid.enabled &= p.eq_enabled;
        let mut high = p.high;
        high.enabled &= p.eq_enabled;
        let disabled = EqBand {
            enabled: false,
            ..EqBand::default()
        };
        self.eq.set_params(&EqParams {
            low_shelf: low,
            peak1: mid,
            peak2: disabled,
            peak3: disabled,
            high_shelf: high,
            ..EqParams::default()
        });
        let side = 1.0 - p.separation;
        let mut row = [(1.0 + side) * 0.5, (1.0 - side) * 0.5];
        if p.swap {
            row.swap(0, 1);
        }
        let left = if p.invert_left { -1.0 } else { 1.0 };
        let right = if p.invert_right { -1.0 } else { 1.0 };
        for (ramp, value) in self.matrix.iter_mut().zip([
            left * row[0],
            left * row[1],
            right * row[1],
            right * row[0],
        ]) {
            if snap {
                ramp.snap(value);
            } else {
                ramp.set_target(value, self.fade);
            }
        }
    }
    pub fn set_params(&mut self, params: TrackParams) {
        let params = params.sanitized();
        if params != self.params {
            self.params = params;
            self.apply(false);
        }
    }
    pub fn automate(&mut self, index: usize, value: f32) {
        let mut params = self.params;
        if params.set(index, value) {
            self.set_params(params);
        }
    }
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.eq.process(left, right);
        for (left, right) in left.iter_mut().zip(right) {
            let l = *left;
            let r = *right;
            let a = self.matrix[0].tick();
            let b = self.matrix[1].tick();
            let c = self.matrix[2].tick();
            let d = self.matrix[3].tick();
            *left = l * a + r * b;
            *right = l * c + r * d;
        }
    }
    pub fn tail_samples(&self) -> usize {
        self.eq.tail_samples()
    }
}
