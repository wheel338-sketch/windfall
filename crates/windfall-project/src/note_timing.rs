//! Non-destructive channel timing shared by playback and MIDI export.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{MAX_PATTERN_TICKS, TICKS_PER_STEP};

pub const MAX_CHANNEL_SHIFT_TICKS: i32 = crate::PPQ as i32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ChannelTiming {
    /// Scales the global swing, 0..=1. One preserves legacy playback.
    pub swing_mix: f32,
    /// Maximum played note duration in ticks; zero leaves lengths unchanged.
    pub gate_ticks: u32,
    /// Translation after swing, bounded to one quarter note in either direction.
    pub shift_ticks: i32,
}

impl Default for ChannelTiming {
    fn default() -> Self {
        Self {
            swing_mix: 1.0,
            gate_ticks: 0,
            shift_ticks: 0,
        }
    }
}

impl ChannelTiming {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.swing_mix.is_finite() || !(0.0..=1.0).contains(&self.swing_mix) {
            return Err("channel swing mix must be between zero and one");
        }
        if self.gate_ticks > MAX_PATTERN_TICKS {
            return Err("channel gate must fit within the longest pattern");
        }
        if self.shift_ticks.abs_diff(0) > MAX_CHANNEL_SHIFT_TICKS as u32 {
            return Err("channel shift must be within one quarter note in either direction");
        }
        Ok(())
    }

    /// Returns the played start and duration without changing stored notes.
    /// Gate caps the duration after swing. Negative shifts clamp the onset
    /// to zero while retaining duration; onsets at/past the pattern end do
    /// not play. Unpaired trailing steps keep the existing swing policy.
    pub fn place(
        &self,
        start: u32,
        duration: u32,
        pattern_length: u32,
        swing: f64,
    ) -> Option<(f64, f64)> {
        if start >= pattern_length {
            return None;
        }
        let mix = if self.swing_mix.is_finite() {
            self.swing_mix.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let swing = if swing.is_finite() {
            swing.clamp(0.0, 1.0) * f64::from(mix)
        } else {
            0.0
        };
        let pair = 2 * TICKS_PER_STEP;
        let swung_length = f64::from(pattern_length / pair * pair);
        let onset = swing_warp(f64::from(start), swing, swung_length);
        let end = swing_warp(
            f64::from(start) + f64::from(duration.max(1)),
            swing,
            swung_length,
        );
        let mut length = (end - onset).max(f64::EPSILON);
        if self.gate_ticks != 0 {
            length = length.min(f64::from(self.gate_ticks.min(MAX_PATTERN_TICKS)));
        }
        let shift = self
            .shift_ticks
            .clamp(-MAX_CHANNEL_SHIFT_TICKS, MAX_CHANNEL_SHIFT_TICKS);
        let tick = (onset + f64::from(shift)).max(0.0);
        (tick < f64::from(pattern_length)).then_some((tick, length))
    }
}

pub fn swing_warp(tick: f64, swing: f64, swung_length: f64) -> f64 {
    if swing <= 0.0 || tick >= swung_length {
        return tick;
    }
    let pair = f64::from(2 * TICKS_PER_STEP);
    let step = f64::from(TICKS_PER_STEP);
    let delay = swing * step / 3.0;
    let pair_start = (tick / pair).floor() * pair;
    let local = tick - pair_start;
    let warped = if local < step {
        local * (step + delay) / step
    } else {
        step + delay + (local - step) * (step - delay) / step
    };
    pair_start + warped
}
