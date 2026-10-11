use super::pattern::{FilterPatternParams, Pattern, patterns_default};
use super::stage::{Stage, input, output, rate, transition_samples};
use crate::blocks::biquad::BiquadCoeffs;
use crate::blocks::smooth::LinearRamp;
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FilterSlotMode {
    #[default]
    Bypass,
    Lowpass,
    Bandpass,
    Highpass,
    /// Active unfiltered branch, for gate-only patterns. Bypass skips a unit.
    Dry,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FilterRouting {
    #[default]
    Serial,
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterSlotParams {
    pub mode: FilterSlotMode,
    /// Corner or center in Hz, 20 to 20,000; clamped below Nyquist.
    pub frequency_hz: f32,
    /// Resonance Q, 0.1 to 10; default 1/sqrt(2).
    pub q: f32,
}
impl Default for FilterSlotParams {
    fn default() -> Self {
        Self {
            mode: FilterSlotMode::Bypass,
            frequency_hz: 1_000.0,
            q: std::f32::consts::FRAC_1_SQRT_2,
        }
    }
}
param_set!(FilterSlotParams, "Filter Slot", {
    choice [mode] "mode" "Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [frequency_hz] "frequencyHz" "Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [q] "q" "Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterBankParams {
    /// Eight independent stereo filter slots; all bypassed by default.
    pub slots: [FilterSlotParams; 8],
    /// Serial cascades all slots. Parallel averages the active branches.
    pub routing: FilterRouting,
    /// Fixed eight-point gate/cutoff curves; absent in legacy/default projects.
    #[serde(skip_serializing_if = "patterns_default")]
    pub patterns: [FilterPatternParams; 8],
}
impl Default for FilterBankParams {
    fn default() -> Self {
        Self {
            slots: [FilterSlotParams::default(); 8],
            routing: FilterRouting::Serial,
            patterns: [FilterPatternParams::default(); 8],
        }
    }
}
param_set!(FilterBankParams, "Filter Bank", {
    choice [slots[0].mode] "slots.0.mode" "Slot 1 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[0].frequency_hz] "slots.0.frequencyHz" "Slot 1 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[0].q] "slots.0.q" "Slot 1 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[1].mode] "slots.1.mode" "Slot 2 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[1].frequency_hz] "slots.1.frequencyHz" "Slot 2 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[1].q] "slots.1.q" "Slot 2 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[2].mode] "slots.2.mode" "Slot 3 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[2].frequency_hz] "slots.2.frequencyHz" "Slot 3 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[2].q] "slots.2.q" "Slot 3 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[3].mode] "slots.3.mode" "Slot 4 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[3].frequency_hz] "slots.3.frequencyHz" "Slot 4 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[3].q] "slots.3.q" "Slot 4 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[4].mode] "slots.4.mode" "Slot 5 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[4].frequency_hz] "slots.4.frequencyHz" "Slot 5 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[4].q] "slots.4.q" "Slot 5 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[5].mode] "slots.5.mode" "Slot 6 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[5].frequency_hz] "slots.5.frequencyHz" "Slot 6 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[5].q] "slots.5.q" "Slot 6 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[6].mode] "slots.6.mode" "Slot 7 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[6].frequency_hz] "slots.6.frequencyHz" "Slot 7 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[6].q] "slots.6.q" "Slot 7 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [slots[7].mode] "slots.7.mode" "Slot 8 Mode" { FilterSlotMode, Bypass, [Bypass "bypass" "Bypass", Lowpass "lowpass" "Lowpass", Bandpass "bandpass" "Bandpass", Highpass "highpass" "Highpass", Dry "dry" "Dry"] }
    float [slots[7].frequency_hz] "slots.7.frequencyHz" "Slot 8 Frequency" { Hertz, Logarithmic, 20.0, 20_000.0, 1_000.0 }
    float [slots[7].q] "slots.7.q" "Slot 8 Q" { Ratio, Logarithmic, 0.1, 10.0, std::f32::consts::FRAC_1_SQRT_2 }
    choice [routing] "routing" "Routing" { FilterRouting, Serial, [Serial "serial" "Serial", Parallel "parallel" "Parallel"] }

    toggle [patterns[0].enabled] "patterns.0.enabled" "Unit 1 pattern" { false }
    float [patterns[0].beats] "patterns.0.beats" "Unit 1 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[0].depth_octaves] "patterns.0.depthOctaves" "Unit 1 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[0].gate[0]] "patterns.0.gate.0" "Unit 1 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[1]] "patterns.0.gate.1" "Unit 1 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[2]] "patterns.0.gate.2" "Unit 1 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[3]] "patterns.0.gate.3" "Unit 1 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[4]] "patterns.0.gate.4" "Unit 1 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[5]] "patterns.0.gate.5" "Unit 1 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[6]] "patterns.0.gate.6" "Unit 1 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].gate[7]] "patterns.0.gate.7" "Unit 1 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[0].cutoff[0]] "patterns.0.cutoff.0" "Unit 1 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[1]] "patterns.0.cutoff.1" "Unit 1 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[2]] "patterns.0.cutoff.2" "Unit 1 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[3]] "patterns.0.cutoff.3" "Unit 1 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[4]] "patterns.0.cutoff.4" "Unit 1 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[5]] "patterns.0.cutoff.5" "Unit 1 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[6]] "patterns.0.cutoff.6" "Unit 1 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[0].cutoff[7]] "patterns.0.cutoff.7" "Unit 1 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[1].enabled] "patterns.1.enabled" "Unit 2 pattern" { false }
    float [patterns[1].beats] "patterns.1.beats" "Unit 2 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[1].depth_octaves] "patterns.1.depthOctaves" "Unit 2 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[1].gate[0]] "patterns.1.gate.0" "Unit 2 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[1]] "patterns.1.gate.1" "Unit 2 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[2]] "patterns.1.gate.2" "Unit 2 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[3]] "patterns.1.gate.3" "Unit 2 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[4]] "patterns.1.gate.4" "Unit 2 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[5]] "patterns.1.gate.5" "Unit 2 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[6]] "patterns.1.gate.6" "Unit 2 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].gate[7]] "patterns.1.gate.7" "Unit 2 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[1].cutoff[0]] "patterns.1.cutoff.0" "Unit 2 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[1]] "patterns.1.cutoff.1" "Unit 2 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[2]] "patterns.1.cutoff.2" "Unit 2 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[3]] "patterns.1.cutoff.3" "Unit 2 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[4]] "patterns.1.cutoff.4" "Unit 2 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[5]] "patterns.1.cutoff.5" "Unit 2 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[6]] "patterns.1.cutoff.6" "Unit 2 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[1].cutoff[7]] "patterns.1.cutoff.7" "Unit 2 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[2].enabled] "patterns.2.enabled" "Unit 3 pattern" { false }
    float [patterns[2].beats] "patterns.2.beats" "Unit 3 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[2].depth_octaves] "patterns.2.depthOctaves" "Unit 3 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[2].gate[0]] "patterns.2.gate.0" "Unit 3 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[1]] "patterns.2.gate.1" "Unit 3 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[2]] "patterns.2.gate.2" "Unit 3 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[3]] "patterns.2.gate.3" "Unit 3 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[4]] "patterns.2.gate.4" "Unit 3 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[5]] "patterns.2.gate.5" "Unit 3 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[6]] "patterns.2.gate.6" "Unit 3 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].gate[7]] "patterns.2.gate.7" "Unit 3 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[2].cutoff[0]] "patterns.2.cutoff.0" "Unit 3 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[1]] "patterns.2.cutoff.1" "Unit 3 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[2]] "patterns.2.cutoff.2" "Unit 3 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[3]] "patterns.2.cutoff.3" "Unit 3 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[4]] "patterns.2.cutoff.4" "Unit 3 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[5]] "patterns.2.cutoff.5" "Unit 3 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[6]] "patterns.2.cutoff.6" "Unit 3 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[2].cutoff[7]] "patterns.2.cutoff.7" "Unit 3 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[3].enabled] "patterns.3.enabled" "Unit 4 pattern" { false }
    float [patterns[3].beats] "patterns.3.beats" "Unit 4 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[3].depth_octaves] "patterns.3.depthOctaves" "Unit 4 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[3].gate[0]] "patterns.3.gate.0" "Unit 4 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[1]] "patterns.3.gate.1" "Unit 4 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[2]] "patterns.3.gate.2" "Unit 4 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[3]] "patterns.3.gate.3" "Unit 4 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[4]] "patterns.3.gate.4" "Unit 4 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[5]] "patterns.3.gate.5" "Unit 4 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[6]] "patterns.3.gate.6" "Unit 4 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].gate[7]] "patterns.3.gate.7" "Unit 4 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[3].cutoff[0]] "patterns.3.cutoff.0" "Unit 4 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[1]] "patterns.3.cutoff.1" "Unit 4 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[2]] "patterns.3.cutoff.2" "Unit 4 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[3]] "patterns.3.cutoff.3" "Unit 4 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[4]] "patterns.3.cutoff.4" "Unit 4 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[5]] "patterns.3.cutoff.5" "Unit 4 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[6]] "patterns.3.cutoff.6" "Unit 4 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[3].cutoff[7]] "patterns.3.cutoff.7" "Unit 4 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[4].enabled] "patterns.4.enabled" "Unit 5 pattern" { false }
    float [patterns[4].beats] "patterns.4.beats" "Unit 5 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[4].depth_octaves] "patterns.4.depthOctaves" "Unit 5 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[4].gate[0]] "patterns.4.gate.0" "Unit 5 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[1]] "patterns.4.gate.1" "Unit 5 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[2]] "patterns.4.gate.2" "Unit 5 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[3]] "patterns.4.gate.3" "Unit 5 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[4]] "patterns.4.gate.4" "Unit 5 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[5]] "patterns.4.gate.5" "Unit 5 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[6]] "patterns.4.gate.6" "Unit 5 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].gate[7]] "patterns.4.gate.7" "Unit 5 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[4].cutoff[0]] "patterns.4.cutoff.0" "Unit 5 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[1]] "patterns.4.cutoff.1" "Unit 5 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[2]] "patterns.4.cutoff.2" "Unit 5 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[3]] "patterns.4.cutoff.3" "Unit 5 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[4]] "patterns.4.cutoff.4" "Unit 5 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[5]] "patterns.4.cutoff.5" "Unit 5 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[6]] "patterns.4.cutoff.6" "Unit 5 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[4].cutoff[7]] "patterns.4.cutoff.7" "Unit 5 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[5].enabled] "patterns.5.enabled" "Unit 6 pattern" { false }
    float [patterns[5].beats] "patterns.5.beats" "Unit 6 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[5].depth_octaves] "patterns.5.depthOctaves" "Unit 6 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[5].gate[0]] "patterns.5.gate.0" "Unit 6 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[1]] "patterns.5.gate.1" "Unit 6 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[2]] "patterns.5.gate.2" "Unit 6 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[3]] "patterns.5.gate.3" "Unit 6 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[4]] "patterns.5.gate.4" "Unit 6 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[5]] "patterns.5.gate.5" "Unit 6 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[6]] "patterns.5.gate.6" "Unit 6 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].gate[7]] "patterns.5.gate.7" "Unit 6 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[5].cutoff[0]] "patterns.5.cutoff.0" "Unit 6 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[1]] "patterns.5.cutoff.1" "Unit 6 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[2]] "patterns.5.cutoff.2" "Unit 6 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[3]] "patterns.5.cutoff.3" "Unit 6 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[4]] "patterns.5.cutoff.4" "Unit 6 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[5]] "patterns.5.cutoff.5" "Unit 6 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[6]] "patterns.5.cutoff.6" "Unit 6 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[5].cutoff[7]] "patterns.5.cutoff.7" "Unit 6 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[6].enabled] "patterns.6.enabled" "Unit 7 pattern" { false }
    float [patterns[6].beats] "patterns.6.beats" "Unit 7 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[6].depth_octaves] "patterns.6.depthOctaves" "Unit 7 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[6].gate[0]] "patterns.6.gate.0" "Unit 7 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[1]] "patterns.6.gate.1" "Unit 7 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[2]] "patterns.6.gate.2" "Unit 7 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[3]] "patterns.6.gate.3" "Unit 7 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[4]] "patterns.6.gate.4" "Unit 7 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[5]] "patterns.6.gate.5" "Unit 7 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[6]] "patterns.6.gate.6" "Unit 7 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].gate[7]] "patterns.6.gate.7" "Unit 7 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[6].cutoff[0]] "patterns.6.cutoff.0" "Unit 7 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[1]] "patterns.6.cutoff.1" "Unit 7 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[2]] "patterns.6.cutoff.2" "Unit 7 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[3]] "patterns.6.cutoff.3" "Unit 7 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[4]] "patterns.6.cutoff.4" "Unit 7 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[5]] "patterns.6.cutoff.5" "Unit 7 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[6]] "patterns.6.cutoff.6" "Unit 7 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[6].cutoff[7]] "patterns.6.cutoff.7" "Unit 7 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
    toggle [patterns[7].enabled] "patterns.7.enabled" "Unit 8 pattern" { false }
    float [patterns[7].beats] "patterns.7.beats" "Unit 8 cycle" { None, Logarithmic, 0.25, 32.0, 4.0 }
    float [patterns[7].depth_octaves] "patterns.7.depthOctaves" "Unit 8 cutoff depth" { None, Linear, 0.0, 8.0, 2.0 }
    float [patterns[7].gate[0]] "patterns.7.gate.0" "Unit 8 gate point 1" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[1]] "patterns.7.gate.1" "Unit 8 gate point 2" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[2]] "patterns.7.gate.2" "Unit 8 gate point 3" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[3]] "patterns.7.gate.3" "Unit 8 gate point 4" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[4]] "patterns.7.gate.4" "Unit 8 gate point 5" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[5]] "patterns.7.gate.5" "Unit 8 gate point 6" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[6]] "patterns.7.gate.6" "Unit 8 gate point 7" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].gate[7]] "patterns.7.gate.7" "Unit 8 gate point 8" { None, Linear, 0.0, 1.0, 1.0 }
    float [patterns[7].cutoff[0]] "patterns.7.cutoff.0" "Unit 8 cutoff point 1" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[1]] "patterns.7.cutoff.1" "Unit 8 cutoff point 2" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[2]] "patterns.7.cutoff.2" "Unit 8 cutoff point 3" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[3]] "patterns.7.cutoff.3" "Unit 8 cutoff point 4" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[4]] "patterns.7.cutoff.4" "Unit 8 cutoff point 5" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[5]] "patterns.7.cutoff.5" "Unit 8 cutoff point 6" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[6]] "patterns.7.cutoff.6" "Unit 8 cutoff point 7" { None, Linear, -1.0, 1.0, 0.0 }
    float [patterns[7].cutoff[7]] "patterns.7.cutoff.7" "Unit 8 cutoff point 8" { None, Linear, -1.0, 1.0, 0.0 }
});

pub(super) fn coefficients(slot: &FilterSlotParams, sample_rate: f32) -> BiquadCoeffs {
    match slot.mode {
        FilterSlotMode::Bypass | FilterSlotMode::Dry => BiquadCoeffs::IDENTITY,
        FilterSlotMode::Lowpass => BiquadCoeffs::low_pass(slot.frequency_hz, slot.q, sample_rate),
        FilterSlotMode::Bandpass => BiquadCoeffs::band_pass(slot.frequency_hz, slot.q, sample_rate),
        FilterSlotMode::Highpass => BiquadCoeffs::high_pass(slot.frequency_hz, slot.q, sample_rate),
    }
}

/// Eight real second-order audio filters. Both routing graphs retain independent
/// histories and run continuously, so routing changes can crossfade over 5 ms.
/// Parallel bypass slots are excluded; an entirely bypassed bank passes dry audio.
pub struct FilterBank {
    sample_rate: f32,
    params: FilterBankParams,
    serial: [Stage; 8],
    parallel: [Stage; 8],
    active: [LinearRamp; 8],
    parallel_mix: LinearRamp,
    fresh: bool,
    patterns: [Pattern; 8],
    gates: [LinearRamp; 8],
    beat_position: f64,
    tempo: f32,
}
impl Default for FilterBank {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            params: FilterBankParams::default(),
            serial: [Stage::default(); 8],
            parallel: [Stage::default(); 8],
            active: [LinearRamp::new(0.0); 8],
            parallel_mix: LinearRamp::new(0.0),
            fresh: true,
            patterns: [Pattern::default(); 8],
            gates: [LinearRamp::new(1.0); 8],
            beat_position: 0.0,
            tempo: 120.0,
        }
    }
}
impl FilterBank {
    fn apply(&mut self) {
        let samples = transition_samples(self.sample_rate, self.fresh);
        for i in 0..8 {
            let slot = self.params.slots[i];
            self.patterns[i].set(slot, self.params.patterns[i], self.sample_rate);
            let patterned = self.patterns[i].enabled() && slot.mode != FilterSlotMode::Bypass;
            let (coeffs, gate) = if patterned {
                self.patterns[i].at(self.beat_position)
            } else {
                (coefficients(&slot, self.sample_rate), 1.0)
            };
            self.gates[i].set_target(gate, samples);
            self.serial[i].set(coeffs, samples);
            self.parallel[i].set(coeffs, samples);
            self.active[i].set_target(
                if slot.mode == FilterSlotMode::Bypass {
                    0.0
                } else {
                    1.0
                },
                samples,
            );
        }
        self.parallel_mix.set_target(
            if self.params.routing == FilterRouting::Parallel {
                1.0
            } else {
                0.0
            },
            samples,
        );
    }
}
impl Effect for FilterBank {
    type Params = FilterBankParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.fresh = true;
        self.beat_position = 0.0;
        self.apply();
        for gate in &mut self.gates {
            gate.snap(gate.target());
        }
        for stage in self.serial.iter_mut().chain(&mut self.parallel) {
            stage.reset();
        }
    }
    fn set_params(&mut self, params: &Self::Params) {
        let params = params.sanitized();
        if params != self.params || self.fresh {
            self.params = params;
            self.apply();
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.tempo = crate::blocks::math::clean(bpm, 1.0, 999.0, 120.0);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let smoothing = transition_samples(self.sample_rate, self.fresh);
            self.fresh = false;
            let dry = [input(*l), input(*r)];
            let mut serial = dry;
            let mut parallel = [0.0; 2];
            let mut weight = 0.0;
            for i in 0..8 {
                if self.patterns[i].enabled() && self.params.slots[i].mode != FilterSlotMode::Bypass
                {
                    let (coeffs, gate) = self.patterns[i].at(self.beat_position);
                    self.serial[i].set(coeffs, smoothing);
                    self.parallel[i].set(coeffs, smoothing);
                    self.gates[i].set_target(gate, smoothing);
                }
                let gate = f64::from(self.gates[i].tick());
                serial = self.serial[i].tick(serial).map(|value| value * gate);
                let branch = self.parallel[i].tick(dry).map(|value| value * gate);
                let active = f64::from(self.active[i].tick());
                weight += active;
                for channel in 0..2 {
                    parallel[channel] += active * branch[channel];
                }
            }
            // A sub-unity active weight blends dry in during the first slot's
            // activation or final slot's deactivation. At rest, this is the mean.
            let dry_weight = (1.0 - weight).max(0.0);
            let normalization = weight.max(1.0);
            let mix = f64::from(self.parallel_mix.tick());
            let mut frame = [0.0; 2];
            for channel in 0..2 {
                let p = (parallel[channel] + dry_weight * dry[channel]) / normalization;
                frame[channel] = if mix == 0.0 {
                    serial[channel]
                } else if mix == 1.0 {
                    p
                } else {
                    serial[channel] + mix * (p - serial[channel])
                };
            }
            self.beat_position += f64::from(self.tempo) / (60.0 * f64::from(self.sample_rate));
            *l = output(frame[0]);
            *r = output(frame[1]);
        }
    }
    fn gap_samples(&self) -> usize {
        // A zero gate can conceal filter memory until the cycle reopens.
        self.tail_samples()
    }
    fn tail_samples(&self) -> usize {
        self.serial
            .iter()
            .map(Stage::tail_samples)
            .sum::<usize>()
            .max(
                self.parallel
                    .iter()
                    .map(Stage::tail_samples)
                    .max()
                    .unwrap_or(0),
            )
            .max(self.patterns.iter().map(Pattern::tail).sum())
    }
}
