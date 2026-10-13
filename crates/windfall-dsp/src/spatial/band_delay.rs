//! Telescoping crossover bands with independent feedback delays/pans.
//! The three-band layout is the original compact circuit and stays the
//! default. The sixteen-band layout splits at fifteen fixed, log-spaced edges
//! of Windfall's own choosing (40 Hz to 16 kHz) and reads its delay, level and
//! pan from one of eight banks; its bands share the three feedback controls
//! by where their centre falls against the two crossover controls.
use super::common::{Controls, Lowpass, Tail, audio, bounded, frames, pole, rate, tap};
use crate::blocks::delay_line::DelayLine;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct BandDelayParams {
    pub low_hz: f32,
    pub high_hz: f32,
    pub time_ms: [f32; 3],
    pub level: [f32; 3],
    pub pan: [f32; 3],
    pub feedback: [f32; 3],
    pub mix: f32,
    pub layout: BandLayout,
    /// The bank the sixteen-band layout plays, counted from 1.
    pub bank: u8,
    pub banks: [BandBank; BANKS],
}

/// How many bands the delay splits into.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BandLayout {
    #[default]
    Three,
    Sixteen,
}

/// Bands of the sixteen-band layout.
pub const WIDE_BANDS: usize = 16;
/// Banks of sixteen-band settings.
pub const BANKS: usize = 8;

/// One bank of the sixteen-band layout.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct BandBank {
    pub time_ms: [f32; WIDE_BANDS],
    pub level: [f32; WIDE_BANDS],
    pub pan: [f32; WIDE_BANDS],
}
impl Default for BandBank {
    fn default() -> Self {
        Self {
            time_ms: [250.0; WIDE_BANDS],
            level: [1.0; WIDE_BANDS],
            pan: [0.0; WIDE_BANDS],
        }
    }
}
impl Default for BandDelayParams {
    fn default() -> Self {
        Self {
            low_hz: 300.0,
            high_hz: 3000.0,
            time_ms: [150.0, 250.0, 375.0],
            level: [1.0; 3],
            pan: [-0.3, 0.0, 0.3],
            feedback: [0.2, 0.3, 0.4],
            mix: 0.3,
            layout: BandLayout::Three,
            bank: 1,
            banks: [BandBank::default(); BANKS],
        }
    }
}
param_set!(BandDelayParams, "Band Delay", {
    float [low_hz] "lowHz" "Low crossover" { Hertz, Logarithmic, 80.0, 1000.0, 300.0 }
    float [high_hz] "highHz" "High crossover" { Hertz, Logarithmic, 1000.0, 10_000.0, 3000.0 }
    float [time_ms[0]] "timeMs.0" "Low delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 150.0 }
    float [time_ms[1]] "timeMs.1" "Mid delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [time_ms[2]] "timeMs.2" "High delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 375.0 }
    float [level[0]] "level.0" "Low level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [level[1]] "level.1" "Mid level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [level[2]] "level.2" "High level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [pan[0]] "pan.0" "Low pan" { Pan, Linear, -1.0, 1.0, -0.3 }
    float [pan[1]] "pan.1" "Mid pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [pan[2]] "pan.2" "High pan" { Pan, Linear, -1.0, 1.0, 0.3 }
    float [feedback[0]] "feedback.0" "Low feedback" { Fraction, Linear, -0.85, 0.85, 0.2 }
    float [feedback[1]] "feedback.1" "Mid feedback" { Fraction, Linear, -0.85, 0.85, 0.3 }
    float [feedback[2]] "feedback.2" "High feedback" { Fraction, Linear, -0.85, 0.85, 0.4 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.3 }
    // Appended for the sixteen-band layout. Rows above keep their indices.
    choice [layout] "layout" "Bands" { BandLayout, Three, [Three "three" "3 bands", Sixteen "sixteen" "16 bands"] }
    int [bank] "bank" "Bank" { None, 1, 8, 1 }
    float [banks[0].time_ms[0]] "banks.0.timeMs.0" "Bank 1 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[1]] "banks.0.timeMs.1" "Bank 1 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[2]] "banks.0.timeMs.2" "Bank 1 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[3]] "banks.0.timeMs.3" "Bank 1 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[4]] "banks.0.timeMs.4" "Bank 1 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[5]] "banks.0.timeMs.5" "Bank 1 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[6]] "banks.0.timeMs.6" "Bank 1 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[7]] "banks.0.timeMs.7" "Bank 1 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[8]] "banks.0.timeMs.8" "Bank 1 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[9]] "banks.0.timeMs.9" "Bank 1 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[10]] "banks.0.timeMs.10" "Bank 1 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[11]] "banks.0.timeMs.11" "Bank 1 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[12]] "banks.0.timeMs.12" "Bank 1 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[13]] "banks.0.timeMs.13" "Bank 1 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[14]] "banks.0.timeMs.14" "Bank 1 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].time_ms[15]] "banks.0.timeMs.15" "Bank 1 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[0].level[0]] "banks.0.level.0" "Bank 1 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[1]] "banks.0.level.1" "Bank 1 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[2]] "banks.0.level.2" "Bank 1 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[3]] "banks.0.level.3" "Bank 1 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[4]] "banks.0.level.4" "Bank 1 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[5]] "banks.0.level.5" "Bank 1 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[6]] "banks.0.level.6" "Bank 1 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[7]] "banks.0.level.7" "Bank 1 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[8]] "banks.0.level.8" "Bank 1 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[9]] "banks.0.level.9" "Bank 1 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[10]] "banks.0.level.10" "Bank 1 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[11]] "banks.0.level.11" "Bank 1 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[12]] "banks.0.level.12" "Bank 1 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[13]] "banks.0.level.13" "Bank 1 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[14]] "banks.0.level.14" "Bank 1 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].level[15]] "banks.0.level.15" "Bank 1 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[0].pan[0]] "banks.0.pan.0" "Bank 1 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[1]] "banks.0.pan.1" "Bank 1 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[2]] "banks.0.pan.2" "Bank 1 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[3]] "banks.0.pan.3" "Bank 1 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[4]] "banks.0.pan.4" "Bank 1 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[5]] "banks.0.pan.5" "Bank 1 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[6]] "banks.0.pan.6" "Bank 1 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[7]] "banks.0.pan.7" "Bank 1 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[8]] "banks.0.pan.8" "Bank 1 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[9]] "banks.0.pan.9" "Bank 1 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[10]] "banks.0.pan.10" "Bank 1 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[11]] "banks.0.pan.11" "Bank 1 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[12]] "banks.0.pan.12" "Bank 1 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[13]] "banks.0.pan.13" "Bank 1 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[14]] "banks.0.pan.14" "Bank 1 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[0].pan[15]] "banks.0.pan.15" "Bank 1 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].time_ms[0]] "banks.1.timeMs.0" "Bank 2 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[1]] "banks.1.timeMs.1" "Bank 2 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[2]] "banks.1.timeMs.2" "Bank 2 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[3]] "banks.1.timeMs.3" "Bank 2 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[4]] "banks.1.timeMs.4" "Bank 2 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[5]] "banks.1.timeMs.5" "Bank 2 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[6]] "banks.1.timeMs.6" "Bank 2 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[7]] "banks.1.timeMs.7" "Bank 2 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[8]] "banks.1.timeMs.8" "Bank 2 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[9]] "banks.1.timeMs.9" "Bank 2 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[10]] "banks.1.timeMs.10" "Bank 2 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[11]] "banks.1.timeMs.11" "Bank 2 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[12]] "banks.1.timeMs.12" "Bank 2 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[13]] "banks.1.timeMs.13" "Bank 2 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[14]] "banks.1.timeMs.14" "Bank 2 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].time_ms[15]] "banks.1.timeMs.15" "Bank 2 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[1].level[0]] "banks.1.level.0" "Bank 2 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[1]] "banks.1.level.1" "Bank 2 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[2]] "banks.1.level.2" "Bank 2 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[3]] "banks.1.level.3" "Bank 2 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[4]] "banks.1.level.4" "Bank 2 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[5]] "banks.1.level.5" "Bank 2 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[6]] "banks.1.level.6" "Bank 2 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[7]] "banks.1.level.7" "Bank 2 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[8]] "banks.1.level.8" "Bank 2 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[9]] "banks.1.level.9" "Bank 2 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[10]] "banks.1.level.10" "Bank 2 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[11]] "banks.1.level.11" "Bank 2 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[12]] "banks.1.level.12" "Bank 2 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[13]] "banks.1.level.13" "Bank 2 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[14]] "banks.1.level.14" "Bank 2 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].level[15]] "banks.1.level.15" "Bank 2 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[1].pan[0]] "banks.1.pan.0" "Bank 2 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[1]] "banks.1.pan.1" "Bank 2 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[2]] "banks.1.pan.2" "Bank 2 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[3]] "banks.1.pan.3" "Bank 2 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[4]] "banks.1.pan.4" "Bank 2 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[5]] "banks.1.pan.5" "Bank 2 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[6]] "banks.1.pan.6" "Bank 2 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[7]] "banks.1.pan.7" "Bank 2 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[8]] "banks.1.pan.8" "Bank 2 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[9]] "banks.1.pan.9" "Bank 2 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[10]] "banks.1.pan.10" "Bank 2 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[11]] "banks.1.pan.11" "Bank 2 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[12]] "banks.1.pan.12" "Bank 2 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[13]] "banks.1.pan.13" "Bank 2 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[14]] "banks.1.pan.14" "Bank 2 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[1].pan[15]] "banks.1.pan.15" "Bank 2 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].time_ms[0]] "banks.2.timeMs.0" "Bank 3 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[1]] "banks.2.timeMs.1" "Bank 3 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[2]] "banks.2.timeMs.2" "Bank 3 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[3]] "banks.2.timeMs.3" "Bank 3 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[4]] "banks.2.timeMs.4" "Bank 3 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[5]] "banks.2.timeMs.5" "Bank 3 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[6]] "banks.2.timeMs.6" "Bank 3 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[7]] "banks.2.timeMs.7" "Bank 3 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[8]] "banks.2.timeMs.8" "Bank 3 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[9]] "banks.2.timeMs.9" "Bank 3 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[10]] "banks.2.timeMs.10" "Bank 3 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[11]] "banks.2.timeMs.11" "Bank 3 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[12]] "banks.2.timeMs.12" "Bank 3 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[13]] "banks.2.timeMs.13" "Bank 3 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[14]] "banks.2.timeMs.14" "Bank 3 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].time_ms[15]] "banks.2.timeMs.15" "Bank 3 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[2].level[0]] "banks.2.level.0" "Bank 3 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[1]] "banks.2.level.1" "Bank 3 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[2]] "banks.2.level.2" "Bank 3 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[3]] "banks.2.level.3" "Bank 3 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[4]] "banks.2.level.4" "Bank 3 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[5]] "banks.2.level.5" "Bank 3 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[6]] "banks.2.level.6" "Bank 3 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[7]] "banks.2.level.7" "Bank 3 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[8]] "banks.2.level.8" "Bank 3 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[9]] "banks.2.level.9" "Bank 3 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[10]] "banks.2.level.10" "Bank 3 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[11]] "banks.2.level.11" "Bank 3 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[12]] "banks.2.level.12" "Bank 3 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[13]] "banks.2.level.13" "Bank 3 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[14]] "banks.2.level.14" "Bank 3 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].level[15]] "banks.2.level.15" "Bank 3 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[2].pan[0]] "banks.2.pan.0" "Bank 3 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[1]] "banks.2.pan.1" "Bank 3 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[2]] "banks.2.pan.2" "Bank 3 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[3]] "banks.2.pan.3" "Bank 3 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[4]] "banks.2.pan.4" "Bank 3 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[5]] "banks.2.pan.5" "Bank 3 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[6]] "banks.2.pan.6" "Bank 3 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[7]] "banks.2.pan.7" "Bank 3 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[8]] "banks.2.pan.8" "Bank 3 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[9]] "banks.2.pan.9" "Bank 3 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[10]] "banks.2.pan.10" "Bank 3 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[11]] "banks.2.pan.11" "Bank 3 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[12]] "banks.2.pan.12" "Bank 3 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[13]] "banks.2.pan.13" "Bank 3 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[14]] "banks.2.pan.14" "Bank 3 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[2].pan[15]] "banks.2.pan.15" "Bank 3 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].time_ms[0]] "banks.3.timeMs.0" "Bank 4 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[1]] "banks.3.timeMs.1" "Bank 4 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[2]] "banks.3.timeMs.2" "Bank 4 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[3]] "banks.3.timeMs.3" "Bank 4 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[4]] "banks.3.timeMs.4" "Bank 4 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[5]] "banks.3.timeMs.5" "Bank 4 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[6]] "banks.3.timeMs.6" "Bank 4 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[7]] "banks.3.timeMs.7" "Bank 4 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[8]] "banks.3.timeMs.8" "Bank 4 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[9]] "banks.3.timeMs.9" "Bank 4 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[10]] "banks.3.timeMs.10" "Bank 4 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[11]] "banks.3.timeMs.11" "Bank 4 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[12]] "banks.3.timeMs.12" "Bank 4 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[13]] "banks.3.timeMs.13" "Bank 4 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[14]] "banks.3.timeMs.14" "Bank 4 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].time_ms[15]] "banks.3.timeMs.15" "Bank 4 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[3].level[0]] "banks.3.level.0" "Bank 4 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[1]] "banks.3.level.1" "Bank 4 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[2]] "banks.3.level.2" "Bank 4 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[3]] "banks.3.level.3" "Bank 4 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[4]] "banks.3.level.4" "Bank 4 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[5]] "banks.3.level.5" "Bank 4 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[6]] "banks.3.level.6" "Bank 4 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[7]] "banks.3.level.7" "Bank 4 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[8]] "banks.3.level.8" "Bank 4 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[9]] "banks.3.level.9" "Bank 4 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[10]] "banks.3.level.10" "Bank 4 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[11]] "banks.3.level.11" "Bank 4 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[12]] "banks.3.level.12" "Bank 4 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[13]] "banks.3.level.13" "Bank 4 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[14]] "banks.3.level.14" "Bank 4 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].level[15]] "banks.3.level.15" "Bank 4 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[3].pan[0]] "banks.3.pan.0" "Bank 4 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[1]] "banks.3.pan.1" "Bank 4 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[2]] "banks.3.pan.2" "Bank 4 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[3]] "banks.3.pan.3" "Bank 4 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[4]] "banks.3.pan.4" "Bank 4 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[5]] "banks.3.pan.5" "Bank 4 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[6]] "banks.3.pan.6" "Bank 4 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[7]] "banks.3.pan.7" "Bank 4 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[8]] "banks.3.pan.8" "Bank 4 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[9]] "banks.3.pan.9" "Bank 4 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[10]] "banks.3.pan.10" "Bank 4 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[11]] "banks.3.pan.11" "Bank 4 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[12]] "banks.3.pan.12" "Bank 4 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[13]] "banks.3.pan.13" "Bank 4 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[14]] "banks.3.pan.14" "Bank 4 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[3].pan[15]] "banks.3.pan.15" "Bank 4 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].time_ms[0]] "banks.4.timeMs.0" "Bank 5 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[1]] "banks.4.timeMs.1" "Bank 5 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[2]] "banks.4.timeMs.2" "Bank 5 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[3]] "banks.4.timeMs.3" "Bank 5 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[4]] "banks.4.timeMs.4" "Bank 5 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[5]] "banks.4.timeMs.5" "Bank 5 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[6]] "banks.4.timeMs.6" "Bank 5 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[7]] "banks.4.timeMs.7" "Bank 5 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[8]] "banks.4.timeMs.8" "Bank 5 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[9]] "banks.4.timeMs.9" "Bank 5 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[10]] "banks.4.timeMs.10" "Bank 5 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[11]] "banks.4.timeMs.11" "Bank 5 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[12]] "banks.4.timeMs.12" "Bank 5 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[13]] "banks.4.timeMs.13" "Bank 5 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[14]] "banks.4.timeMs.14" "Bank 5 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].time_ms[15]] "banks.4.timeMs.15" "Bank 5 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[4].level[0]] "banks.4.level.0" "Bank 5 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[1]] "banks.4.level.1" "Bank 5 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[2]] "banks.4.level.2" "Bank 5 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[3]] "banks.4.level.3" "Bank 5 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[4]] "banks.4.level.4" "Bank 5 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[5]] "banks.4.level.5" "Bank 5 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[6]] "banks.4.level.6" "Bank 5 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[7]] "banks.4.level.7" "Bank 5 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[8]] "banks.4.level.8" "Bank 5 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[9]] "banks.4.level.9" "Bank 5 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[10]] "banks.4.level.10" "Bank 5 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[11]] "banks.4.level.11" "Bank 5 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[12]] "banks.4.level.12" "Bank 5 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[13]] "banks.4.level.13" "Bank 5 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[14]] "banks.4.level.14" "Bank 5 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].level[15]] "banks.4.level.15" "Bank 5 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[4].pan[0]] "banks.4.pan.0" "Bank 5 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[1]] "banks.4.pan.1" "Bank 5 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[2]] "banks.4.pan.2" "Bank 5 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[3]] "banks.4.pan.3" "Bank 5 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[4]] "banks.4.pan.4" "Bank 5 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[5]] "banks.4.pan.5" "Bank 5 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[6]] "banks.4.pan.6" "Bank 5 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[7]] "banks.4.pan.7" "Bank 5 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[8]] "banks.4.pan.8" "Bank 5 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[9]] "banks.4.pan.9" "Bank 5 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[10]] "banks.4.pan.10" "Bank 5 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[11]] "banks.4.pan.11" "Bank 5 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[12]] "banks.4.pan.12" "Bank 5 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[13]] "banks.4.pan.13" "Bank 5 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[14]] "banks.4.pan.14" "Bank 5 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[4].pan[15]] "banks.4.pan.15" "Bank 5 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].time_ms[0]] "banks.5.timeMs.0" "Bank 6 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[1]] "banks.5.timeMs.1" "Bank 6 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[2]] "banks.5.timeMs.2" "Bank 6 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[3]] "banks.5.timeMs.3" "Bank 6 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[4]] "banks.5.timeMs.4" "Bank 6 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[5]] "banks.5.timeMs.5" "Bank 6 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[6]] "banks.5.timeMs.6" "Bank 6 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[7]] "banks.5.timeMs.7" "Bank 6 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[8]] "banks.5.timeMs.8" "Bank 6 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[9]] "banks.5.timeMs.9" "Bank 6 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[10]] "banks.5.timeMs.10" "Bank 6 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[11]] "banks.5.timeMs.11" "Bank 6 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[12]] "banks.5.timeMs.12" "Bank 6 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[13]] "banks.5.timeMs.13" "Bank 6 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[14]] "banks.5.timeMs.14" "Bank 6 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].time_ms[15]] "banks.5.timeMs.15" "Bank 6 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[5].level[0]] "banks.5.level.0" "Bank 6 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[1]] "banks.5.level.1" "Bank 6 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[2]] "banks.5.level.2" "Bank 6 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[3]] "banks.5.level.3" "Bank 6 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[4]] "banks.5.level.4" "Bank 6 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[5]] "banks.5.level.5" "Bank 6 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[6]] "banks.5.level.6" "Bank 6 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[7]] "banks.5.level.7" "Bank 6 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[8]] "banks.5.level.8" "Bank 6 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[9]] "banks.5.level.9" "Bank 6 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[10]] "banks.5.level.10" "Bank 6 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[11]] "banks.5.level.11" "Bank 6 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[12]] "banks.5.level.12" "Bank 6 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[13]] "banks.5.level.13" "Bank 6 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[14]] "banks.5.level.14" "Bank 6 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].level[15]] "banks.5.level.15" "Bank 6 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[5].pan[0]] "banks.5.pan.0" "Bank 6 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[1]] "banks.5.pan.1" "Bank 6 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[2]] "banks.5.pan.2" "Bank 6 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[3]] "banks.5.pan.3" "Bank 6 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[4]] "banks.5.pan.4" "Bank 6 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[5]] "banks.5.pan.5" "Bank 6 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[6]] "banks.5.pan.6" "Bank 6 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[7]] "banks.5.pan.7" "Bank 6 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[8]] "banks.5.pan.8" "Bank 6 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[9]] "banks.5.pan.9" "Bank 6 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[10]] "banks.5.pan.10" "Bank 6 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[11]] "banks.5.pan.11" "Bank 6 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[12]] "banks.5.pan.12" "Bank 6 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[13]] "banks.5.pan.13" "Bank 6 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[14]] "banks.5.pan.14" "Bank 6 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[5].pan[15]] "banks.5.pan.15" "Bank 6 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].time_ms[0]] "banks.6.timeMs.0" "Bank 7 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[1]] "banks.6.timeMs.1" "Bank 7 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[2]] "banks.6.timeMs.2" "Bank 7 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[3]] "banks.6.timeMs.3" "Bank 7 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[4]] "banks.6.timeMs.4" "Bank 7 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[5]] "banks.6.timeMs.5" "Bank 7 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[6]] "banks.6.timeMs.6" "Bank 7 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[7]] "banks.6.timeMs.7" "Bank 7 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[8]] "banks.6.timeMs.8" "Bank 7 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[9]] "banks.6.timeMs.9" "Bank 7 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[10]] "banks.6.timeMs.10" "Bank 7 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[11]] "banks.6.timeMs.11" "Bank 7 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[12]] "banks.6.timeMs.12" "Bank 7 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[13]] "banks.6.timeMs.13" "Bank 7 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[14]] "banks.6.timeMs.14" "Bank 7 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].time_ms[15]] "banks.6.timeMs.15" "Bank 7 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[6].level[0]] "banks.6.level.0" "Bank 7 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[1]] "banks.6.level.1" "Bank 7 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[2]] "banks.6.level.2" "Bank 7 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[3]] "banks.6.level.3" "Bank 7 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[4]] "banks.6.level.4" "Bank 7 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[5]] "banks.6.level.5" "Bank 7 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[6]] "banks.6.level.6" "Bank 7 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[7]] "banks.6.level.7" "Bank 7 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[8]] "banks.6.level.8" "Bank 7 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[9]] "banks.6.level.9" "Bank 7 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[10]] "banks.6.level.10" "Bank 7 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[11]] "banks.6.level.11" "Bank 7 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[12]] "banks.6.level.12" "Bank 7 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[13]] "banks.6.level.13" "Bank 7 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[14]] "banks.6.level.14" "Bank 7 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].level[15]] "banks.6.level.15" "Bank 7 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[6].pan[0]] "banks.6.pan.0" "Bank 7 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[1]] "banks.6.pan.1" "Bank 7 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[2]] "banks.6.pan.2" "Bank 7 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[3]] "banks.6.pan.3" "Bank 7 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[4]] "banks.6.pan.4" "Bank 7 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[5]] "banks.6.pan.5" "Bank 7 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[6]] "banks.6.pan.6" "Bank 7 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[7]] "banks.6.pan.7" "Bank 7 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[8]] "banks.6.pan.8" "Bank 7 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[9]] "banks.6.pan.9" "Bank 7 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[10]] "banks.6.pan.10" "Bank 7 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[11]] "banks.6.pan.11" "Bank 7 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[12]] "banks.6.pan.12" "Bank 7 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[13]] "banks.6.pan.13" "Bank 7 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[14]] "banks.6.pan.14" "Bank 7 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[6].pan[15]] "banks.6.pan.15" "Bank 7 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].time_ms[0]] "banks.7.timeMs.0" "Bank 8 band 1 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[1]] "banks.7.timeMs.1" "Bank 8 band 2 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[2]] "banks.7.timeMs.2" "Bank 8 band 3 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[3]] "banks.7.timeMs.3" "Bank 8 band 4 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[4]] "banks.7.timeMs.4" "Bank 8 band 5 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[5]] "banks.7.timeMs.5" "Bank 8 band 6 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[6]] "banks.7.timeMs.6" "Bank 8 band 7 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[7]] "banks.7.timeMs.7" "Bank 8 band 8 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[8]] "banks.7.timeMs.8" "Bank 8 band 9 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[9]] "banks.7.timeMs.9" "Bank 8 band 10 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[10]] "banks.7.timeMs.10" "Bank 8 band 11 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[11]] "banks.7.timeMs.11" "Bank 8 band 12 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[12]] "banks.7.timeMs.12" "Bank 8 band 13 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[13]] "banks.7.timeMs.13" "Bank 8 band 14 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[14]] "banks.7.timeMs.14" "Bank 8 band 15 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].time_ms[15]] "banks.7.timeMs.15" "Bank 8 band 16 delay" { Milliseconds, Logarithmic, 1.0, 1000.0, 250.0 }
    float [banks[7].level[0]] "banks.7.level.0" "Bank 8 band 1 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[1]] "banks.7.level.1" "Bank 8 band 2 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[2]] "banks.7.level.2" "Bank 8 band 3 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[3]] "banks.7.level.3" "Bank 8 band 4 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[4]] "banks.7.level.4" "Bank 8 band 5 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[5]] "banks.7.level.5" "Bank 8 band 6 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[6]] "banks.7.level.6" "Bank 8 band 7 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[7]] "banks.7.level.7" "Bank 8 band 8 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[8]] "banks.7.level.8" "Bank 8 band 9 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[9]] "banks.7.level.9" "Bank 8 band 10 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[10]] "banks.7.level.10" "Bank 8 band 11 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[11]] "banks.7.level.11" "Bank 8 band 12 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[12]] "banks.7.level.12" "Bank 8 band 13 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[13]] "banks.7.level.13" "Bank 8 band 14 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[14]] "banks.7.level.14" "Bank 8 band 15 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].level[15]] "banks.7.level.15" "Bank 8 band 16 level" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [banks[7].pan[0]] "banks.7.pan.0" "Bank 8 band 1 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[1]] "banks.7.pan.1" "Bank 8 band 2 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[2]] "banks.7.pan.2" "Bank 8 band 3 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[3]] "banks.7.pan.3" "Bank 8 band 4 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[4]] "banks.7.pan.4" "Bank 8 band 5 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[5]] "banks.7.pan.5" "Bank 8 band 6 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[6]] "banks.7.pan.6" "Bank 8 band 7 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[7]] "banks.7.pan.7" "Bank 8 band 8 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[8]] "banks.7.pan.8" "Bank 8 band 9 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[9]] "banks.7.pan.9" "Bank 8 band 10 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[10]] "banks.7.pan.10" "Bank 8 band 11 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[11]] "banks.7.pan.11" "Bank 8 band 12 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[12]] "banks.7.pan.12" "Bank 8 band 13 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[13]] "banks.7.pan.13" "Bank 8 band 14 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[14]] "banks.7.pan.14" "Bank 8 band 15 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
    float [banks[7].pan[15]] "banks.7.pan.15" "Bank 8 band 16 pan" { Pan, Linear, -1.0, 1.0, 0.0 }
});
/// Smoothed controls of the sixteen-band layout: mix, three feedbacks, then
/// sixteen delays, levels and pans of the playing bank.
const WIDE_CONTROLS: usize = 4 + 3 * WIDE_BANDS;
/// The lowest and highest of the fifteen band edges; the rest are spaced by
/// equal ratios between them.
const WIDE_EDGE_HZ: (f32, f32) = (40.0, 16_000.0);

/// The upper edge of `band` in the sixteen-band layout. The top band has none.
pub fn wide_edge_hz(band: usize) -> f32 {
    let (low, high) = WIDE_EDGE_HZ;
    low * (high / low).powf(band.min(WIDE_BANDS - 2) as f32 / (WIDE_BANDS - 2) as f32)
}

/// The geometric centre of `band`, which decides whose feedback it uses.
fn wide_centre_hz(band: usize) -> f32 {
    let step = (WIDE_EDGE_HZ.1 / WIDE_EDGE_HZ.0).powf(1.0 / (WIDE_BANDS - 2) as f32);
    wide_edge_hz(band.min(WIDE_BANDS - 2)) / step.sqrt()
        * if band == WIDE_BANDS - 1 { step } else { 1.0 }
}

pub struct BandDelay {
    params: BandDelayParams,
    controls: Controls<15>,
    wide: Controls<WIDE_CONTROLS>,
    crossovers: [[[Lowpass; 2]; 2]; 2],
    wide_crossovers: [[[Lowpass; 2]; WIDE_BANDS - 1]; 2],
    wide_poles: [f32; WIDE_BANDS - 1],
    /// Which of the three feedback controls each wide band follows.
    groups: [usize; WIDE_BANDS],
    lines: [[DelayLine; 2]; WIDE_BANDS],
    tone: [[Lowpass; 2]; WIDE_BANDS],
    rate: f32,
    tail: Tail,
    prepared: bool,
    /// The layout that is sounding. A change fades the echoes out, clears
    /// them and fades the new layout in, so stale bands never play.
    active: BandLayout,
    fade_out: u32,
    fade_in: u32,
    fade: u32,
    started: bool,
}
impl Default for BandDelay {
    fn default() -> Self {
        Self {
            params: BandDelayParams::default(),
            controls: Controls::default(),
            wide: Controls::default(),
            crossovers: [[[Lowpass::default(); 2]; 2]; 2],
            wide_crossovers: [[[Lowpass::default(); 2]; WIDE_BANDS - 1]; 2],
            wide_poles: [0.0; WIDE_BANDS - 1],
            groups: [0; WIDE_BANDS],
            lines: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::default())),
            tone: [[Lowpass::default(); 2]; WIDE_BANDS],
            rate: 48_000.0,
            tail: Tail::default(),
            prepared: false,
            active: BandLayout::Three,
            fade_out: 0,
            fade_in: 0,
            fade: 240,
            started: false,
        }
    }
}
impl BandDelay {
    fn apply(&mut self) {
        let p = self.params;
        self.controls.set([
            p.low_hz.ln(),
            p.high_hz.ln(),
            p.time_ms[0],
            p.time_ms[1],
            p.time_ms[2],
            p.level[0],
            p.level[1],
            p.level[2],
            p.pan[0],
            p.pan[1],
            p.pan[2],
            p.feedback[0],
            p.feedback[1],
            p.feedback[2],
            p.mix,
        ]);
        let bank = p.banks[usize::from(p.bank.clamp(1, BANKS as u8) - 1)];
        let mut wide = [0.0; WIDE_CONTROLS];
        wide[0] = p.mix;
        wide[1..4].copy_from_slice(&p.feedback);
        wide[4..4 + WIDE_BANDS].copy_from_slice(&bank.time_ms);
        wide[4 + WIDE_BANDS..4 + 2 * WIDE_BANDS].copy_from_slice(&bank.level);
        wide[4 + 2 * WIDE_BANDS..].copy_from_slice(&bank.pan);
        self.wide.set(wide);
        for (band, group) in self.groups.iter_mut().enumerate() {
            let centre = wide_centre_hz(band);
            *group = usize::from(centre >= p.low_hz) + usize::from(centre >= p.high_hz);
        }
        if !self.started {
            self.active = p.layout;
        } else if p.layout != self.active && self.fade_out == 0 {
            self.fade_out = self.fade;
        }
    }
    fn clear(&mut self) {
        for line in self.lines.iter_mut().flatten() {
            line.clear();
        }
        self.crossovers = [[[Lowpass::default(); 2]; 2]; 2];
        self.wide_crossovers = [[[Lowpass::default(); 2]; WIDE_BANDS - 1]; 2];
        self.tone = [[Lowpass::default(); 2]; WIDE_BANDS];
    }
    /// One frame of the original three-band circuit.
    fn three(&mut self, dry: [f32; 2]) -> ([f32; 2], f32) {
        let c = self.controls.tick();
        let poles = [pole(c[0].exp(), self.rate), pole(c[1].exp(), self.rate)];
        let tone = pole(7000.0, self.rate);
        let mut wet = [0.0; 2];
        for ch in 0..2 {
            let mut cumulative = [0.0; 2];
            for crossover in 0..2 {
                let mut x = dry[ch];
                for section in &mut self.crossovers[ch][crossover] {
                    x = section.tick(x, poles[crossover]);
                }
                cumulative[crossover] = x;
            }
            // These sum exactly to the input before independent delays.
            let bands = [
                cumulative[0],
                cumulative[1] - cumulative[0],
                dry[ch] - cumulative[1],
            ];
            for band in 0..3 {
                let echo = tap(
                    &self.lines[band][ch],
                    super::common::samples(c[2 + band], self.rate),
                );
                let back = self.tone[band][ch].tick(echo, tone);
                self.lines[band][ch].push(bounded(bands[band] + c[11 + band] * back));
                let pan = c[8 + band];
                let balance = if ch == 0 {
                    (1.0 - pan).min(1.0)
                } else {
                    (1.0 + pan).min(1.0)
                };
                wet[ch] += echo * c[5 + band] * balance;
            }
        }
        (wet, c[14])
    }
    /// One frame of the sixteen-band layout. Each band is the difference of
    /// two neighbouring lowpassed copies, so the bands still sum to the input.
    fn sixteen(&mut self, dry: [f32; 2]) -> ([f32; 2], f32) {
        let c = self.wide.tick();
        let tone = pole(7000.0, self.rate);
        let mut wet = [0.0; 2];
        for ch in 0..2 {
            let mut below = 0.0;
            for band in 0..WIDE_BANDS {
                let upto = if band < WIDE_BANDS - 1 {
                    let mut x = dry[ch];
                    for section in &mut self.wide_crossovers[ch][band] {
                        x = section.tick(x, self.wide_poles[band]);
                    }
                    x
                } else {
                    dry[ch]
                };
                let input = upto - below;
                below = upto;
                let echo = tap(
                    &self.lines[band][ch],
                    super::common::samples(c[4 + band], self.rate),
                );
                let back = self.tone[band][ch].tick(echo, tone);
                self.lines[band][ch].push(bounded(input + c[1 + self.groups[band]] * back));
                let pan = c[4 + 2 * WIDE_BANDS + band];
                let balance = if ch == 0 {
                    (1.0 - pan).min(1.0)
                } else {
                    (1.0 + pan).min(1.0)
                };
                wet[ch] += echo * c[4 + WIDE_BANDS + band] * balance;
            }
        }
        (wet, c[0])
    }
}
impl Effect for BandDelay {
    type Params = BandDelayParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.rate = rate(sample_rate);
        self.lines = std::array::from_fn(|_| {
            std::array::from_fn(|_| DelayLine::new(frames(1000.0, self.rate) + 2))
        });
        self.wide_poles = std::array::from_fn(|band| pole(wide_edge_hz(band), self.rate));
        self.controls.prepare(self.rate);
        self.wide.prepare(self.rate);
        self.tail.prepare(90.0, self.rate);
        self.fade = frames(5.0, self.rate) as u32;
        self.prepared = true;
        self.reset();
    }
    fn reset(&mut self) {
        self.clear();
        self.controls.reset();
        self.wide.reset();
        self.tail.reset();
        self.started = false;
        self.fade_out = 0;
        self.fade_in = 0;
        self.apply();
    }
    fn set_params(&mut self, p: &Self::Params) {
        self.params = p.sanitized();
        self.apply();
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let dry = [audio(*l), audio(*r)];
            if !self.prepared {
                [*l, *r] = dry;
                continue;
            }
            self.started = true;
            let (gain, expired) = self.tail.tick(dry);
            let (mut wet, mix) = match self.active {
                BandLayout::Three => self.three(dry),
                BandLayout::Sixteen => self.sixteen(dry),
            };
            if self.fade_out > 0 {
                self.fade_out -= 1;
                let level = self.fade_out as f32 / self.fade as f32;
                wet = [wet[0] * level, wet[1] * level];
                if self.fade_out == 0 {
                    self.clear();
                    self.active = self.params.layout;
                    self.fade_in = self.fade;
                }
            } else if self.fade_in > 0 {
                self.fade_in -= 1;
                let level = 1.0 - self.fade_in as f32 / self.fade as f32;
                wet = [wet[0] * level, wet[1] * level];
            }
            if expired {
                self.clear();
            }
            *l = bounded(dry[0] + mix * (gain * wet[0] - dry[0]));
            *r = bounded(dry[1] + mix * (gain * wet[1] - dry[1]));
        }
    }
    fn tail_samples(&self) -> usize {
        self.tail.samples()
    }
    fn gap_samples(&self) -> usize {
        self.tail_samples()
    }
    fn warm_up_samples(&self) -> usize {
        frames(1000.0, self.rate)
    }
    fn delay_readiness_samples(&self) -> usize {
        self.warm_up_samples()
    }
}
