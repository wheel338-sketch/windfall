//! Bounded, repeating drawn envelopes for each filter unit. All coefficient
//! endpoints are prepared on parameter writes; the sample path only interpolates.
use super::filter_bank::{FilterSlotParams, coefficients};
use crate::blocks::biquad::BiquadCoeffs;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const FILTER_PATTERN_POINTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterPatternParams {
    pub enabled: bool,
    /// Quarter notes per repeating cycle, independent for each unit.
    pub beats: f32,
    /// Bipolar cutoff points scale this many octaves around the base frequency.
    pub depth_octaves: f32,
    pub gate: [f32; FILTER_PATTERN_POINTS],
    pub cutoff: [f32; FILTER_PATTERN_POINTS],
}
impl Default for FilterPatternParams {
    fn default() -> Self {
        Self {
            enabled: false,
            beats: 4.0,
            depth_octaves: 2.0,
            gate: [1.0; FILTER_PATTERN_POINTS],
            cutoff: [0.0; FILTER_PATTERN_POINTS],
        }
    }
}
pub(super) fn patterns_default(patterns: &[FilterPatternParams; 8]) -> bool {
    *patterns == [FilterPatternParams::default(); 8]
}

/// Linear interpolation wraps the final point back to the first.
pub(super) fn location(beats: f64, period: f32) -> (usize, usize, f64) {
    let position = (beats / f64::from(period)).rem_euclid(1.0) * FILTER_PATTERN_POINTS as f64;
    let first = position.floor() as usize;
    (
        first,
        (first + 1) % FILTER_PATTERN_POINTS,
        position - first as f64,
    )
}
fn blend(a: f64, b: f64, share: f64) -> f64 {
    a + share * (b - a)
}

#[derive(Clone, Copy)]
pub(super) struct Pattern {
    settings: FilterPatternParams,
    table: [BiquadCoeffs; FILTER_PATTERN_POINTS],
    tail: usize,
}
impl Default for Pattern {
    fn default() -> Self {
        Self {
            settings: FilterPatternParams::default(),
            table: [BiquadCoeffs::IDENTITY; FILTER_PATTERN_POINTS],
            tail: 0,
        }
    }
}
impl Pattern {
    pub(super) fn set(&mut self, slot: FilterSlotParams, settings: FilterPatternParams, rate: f32) {
        self.settings = settings;
        self.table = std::array::from_fn(|i| {
            coefficients(
                &FilterSlotParams {
                    frequency_hz: (slot.frequency_hz
                        * 2.0_f32.powf(settings.cutoff[i] * settings.depth_octaves))
                    .clamp(20.0, 20_000.0),
                    ..slot
                },
                rate,
            )
        });
        self.tail = self
            .table
            .iter()
            .map(super::stage::coefficient_tail)
            .max()
            .unwrap_or(0);
    }
    pub(super) fn enabled(&self) -> bool {
        self.settings.enabled
    }
    pub(super) fn tail(&self) -> usize {
        if self.enabled() { self.tail } else { 0 }
    }
    pub(super) fn at(&self, beats: f64) -> (BiquadCoeffs, f32) {
        let (i, j, share) = location(beats, self.settings.beats);
        let (a, b) = (self.table[i], self.table[j]);
        (
            BiquadCoeffs {
                b0: blend(a.b0, b.b0, share),
                b1: blend(a.b1, b.b1, share),
                b2: blend(a.b2, b.b2, share),
                a1: blend(a.a1, b.a1, share),
                a2: blend(a.a2, b.a2, share),
            },
            blend(
                f64::from(self.settings.gate[i]),
                f64::from(self.settings.gate[j]),
                share,
            ) as f32,
        )
    }
}
