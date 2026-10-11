use crate::asset::CurveRef;
use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct VolumeGateParams {
    /// A rising edge captures preceding audio and restarts both envelopes.
    /// While held the captured slice is immutable; release resumes live capture.
    pub trigger: bool,
    #[ts(as = "Vec<crate::asset::CurvePoint>")]
    pub time_curve: CurveRef,
    #[ts(as = "Vec<crate::asset::CurvePoint>")]
    pub volume_curve: CurveRef,
    /// Sixteen linear gains, 0..=1; default unity. These are the volume curve.
    pub steps: [f32; 16],
    /// Quarter-note beats per cycle, 0.125..=16; default 4. Capped at 2 seconds.
    pub loop_beats: f32,
    /// Probability per step boundary of replaying preceding audio, 0..=1.
    pub retrigger_chance: f32,
    /// Number of preceding steps to jump back, 1..=16; default 1.
    pub retrigger_steps: u8,
    /// Recirculated wet signal, 0..=0.95; default 0.
    pub feedback: f32,
    /// Dry/wet blend, 0..=1; default 1.
    pub mix: f32,
}

impl Default for VolumeGateParams {
    fn default() -> Self {
        Self {
            trigger: false,
            time_curve: CurveRef::NONE,
            volume_curve: CurveRef::NONE,
            steps: [1.0; 16],
            loop_beats: 4.0,
            retrigger_chance: 0.0,
            retrigger_steps: 1,
            feedback: 0.0,
            mix: 1.0,
        }
    }
}

param_set!(VolumeGateParams, "Volume Gate", {
    float [steps[0]] "steps.0" "Step 1" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[1]] "steps.1" "Step 2" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[2]] "steps.2" "Step 3" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[3]] "steps.3" "Step 4" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[4]] "steps.4" "Step 5" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[5]] "steps.5" "Step 6" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[6]] "steps.6" "Step 7" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[7]] "steps.7" "Step 8" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[8]] "steps.8" "Step 9" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[9]] "steps.9" "Step 10" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[10]] "steps.10" "Step 11" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[11]] "steps.11" "Step 12" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[12]] "steps.12" "Step 13" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[13]] "steps.13" "Step 14" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[14]] "steps.14" "Step 15" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [steps[15]] "steps.15" "Step 16" { Gain, Linear, 0.0, 1.0, 1.0 }
    float [loop_beats] "loopBeats" "Loop" { None, Linear, 0.125, 16.0, 4.0 }
    float [retrigger_chance] "retriggerChance" "Retrigger Chance" { Fraction, Linear, 0.0, 1.0, 0.0 }
    int [retrigger_steps] "retriggerSteps" "Retrigger Steps" { None, 1, 16, 1 }
    float [feedback] "feedback" "Feedback" { Fraction, Linear, 0.0, 0.95, 0.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [trigger] "trigger" "Trigger" { false }
});

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum TransportMode {
    Hold,
    Reverse,
    #[default]
    Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct TimeTransportParams {
    /// Automatic onset capture/reloop. Manual Trigger remains available.
    pub auto_trigger: bool,
    pub threshold_db: f32,
    pub sensitivity: f32,
    pub refractory_ms: f32,
    /// False records/passes live audio; a rising edge captures preceding audio.
    pub trigger: bool,
    pub mode: TransportMode,
    /// Capture length in quarter-note beats, 0.125..=16; default 1, capped at 2 s.
    pub loop_beats: f32,
    /// Slice playback rate, 0.25..=4; default 1 (ignored in Hold).
    pub rate: f32,
    /// Dry/wet blend, 0..=1; default 1.
    pub mix: f32,
}
impl Default for TimeTransportParams {
    fn default() -> Self {
        Self {
            auto_trigger: false,
            threshold_db: -24.0,
            sensitivity: 1.8,
            refractory_ms: 50.0,
            trigger: false,
            mode: TransportMode::Repeat,
            loop_beats: 1.0,
            rate: 1.0,
            mix: 1.0,
        }
    }
}
param_set!(TimeTransportParams, "Time Transport", {
    toggle [trigger] "trigger" "Trigger" { false }
    choice [mode] "mode" "Mode" { TransportMode, Repeat, [Hold "hold" "Hold", Reverse "reverse" "Reverse", Repeat "repeat" "Repeat"] }
    float [loop_beats] "loopBeats" "Slice" { None, Linear, 0.125, 16.0, 1.0 }
    float [rate] "rate" "Rate" { Ratio, Logarithmic, 0.25, 4.0, 1.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [auto_trigger] "autoTrigger" "Transient capture" { false }
    float [threshold_db] "thresholdDb" "Onset threshold" { Decibels, Linear, -60.0, 0.0, -24.0 }
    float [sensitivity] "sensitivity" "Onset contrast" { Ratio, Linear, 1.1, 8.0, 1.8 }
    float [refractory_ms] "refractoryMs" "Retrigger holdoff" { Milliseconds, Linear, 1.0, 500.0, 50.0 }
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ScratchParams {
    /// Project sample id; 0 retains the legacy live record.
    pub source_id: u32,
    /// Compiled allocation identity, never saved or exposed over IPC.
    #[serde(skip)]
    #[ts(skip)]
    pub runtime_source: crate::asset::SourceRef,
    #[ts(as = "Vec<crate::asset::CurvePoint>")]
    pub position_curve: CurveRef,
    /// Automatically follow the drawn position curve, synced to Record beats.
    pub curve_enabled: bool,
    /// 0 is oldest, 1 newest, within the recent loop. Default 1.
    pub position: f32,
    /// Freeze the recording so position addresses a stable record. Default false.
    pub freeze: bool,
    /// Quarter-note beats in the record, 0.125..=16; default 4, capped at 2 s.
    pub loop_beats: f32,
    /// Dry/wet blend, 0..=1; default 1.
    pub mix: f32,
}
impl Default for ScratchParams {
    fn default() -> Self {
        Self {
            source_id: 0,
            runtime_source: crate::asset::SourceRef::NONE,
            position_curve: CurveRef::NONE,
            curve_enabled: false,
            position: 1.0,
            freeze: false,
            loop_beats: 4.0,
            mix: 1.0,
        }
    }
}
param_set!(ScratchParams, "Scratch", {
    float [position] "position" "Position" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [freeze] "freeze" "Freeze" { false }
    float [loop_beats] "loopBeats" "Record" { None, Linear, 0.125, 16.0, 4.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [curve_enabled] "curveEnabled" "Follow scratch curve" { false }
});

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum PerformanceModel {
    #[default]
    Stutter,
    Reverse,
    TapeStop,
    HalfSpeed,
    DoubleSpeed,
    BeatRepeat,
    TelephoneFilter,
    Distortion,
    PanSpin,
    Fade,
    SilenceGate,
    PitchJump,
}
impl PerformanceModel {
    pub const ALL: [Self; 12] = [
        Self::Stutter,
        Self::Reverse,
        Self::TapeStop,
        Self::HalfSpeed,
        Self::DoubleSpeed,
        Self::BeatRepeat,
        Self::TelephoneFilter,
        Self::Distortion,
        Self::PanSpin,
        Self::Fade,
        Self::SilenceGate,
        Self::PitchJump,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct PerformanceRackParams {
    /// With XY enabled, X controls amount and Y the model's secondary axis.
    pub xy_enabled: bool,
    pub x: f32,
    pub y: f32,
    pub model: PerformanceModel,
    /// Quarter-note beats in each captured slice, 0.125..=16; default 1.
    /// Capped at 1 second to leave an equally sized recording region.
    pub loop_beats: f32,
    /// Treatment intensity, 0..=1; default 1. See the integration seam.
    pub amount: f32,
    /// Pitch Jump transposition, -24..=24 semitones; default 12.
    pub pitch_semitones: f32,
    /// Wet feedback written into the recording region, 0..=0.95; default 0.
    pub feedback: f32,
    /// Blend with the latency-aligned dry signal, 0..=1; default 1.
    pub mix: f32,
}
impl Default for PerformanceRackParams {
    fn default() -> Self {
        Self {
            xy_enabled: false,
            x: 1.0,
            y: 0.5,
            model: PerformanceModel::Stutter,
            loop_beats: 1.0,
            amount: 1.0,
            pitch_semitones: 12.0,
            feedback: 0.0,
            mix: 1.0,
        }
    }
}
param_set!(PerformanceRackParams, "Performance Rack", {
    choice [model] "model" "Treatment" { PerformanceModel, Stutter, [
        Stutter "stutter" "Stutter", Reverse "reverse" "Reverse",
        TapeStop "tapeStop" "Tape Stop", HalfSpeed "halfSpeed" "Half Speed",
        DoubleSpeed "doubleSpeed" "Double Speed", BeatRepeat "beatRepeat" "Beat Repeat",
        TelephoneFilter "telephoneFilter" "Telephone Filter", Distortion "distortion" "Distortion",
        PanSpin "panSpin" "Pan Spin", Fade "fade" "Fade", SilenceGate "silenceGate" "Silence Gate",
        PitchJump "pitchJump" "Pitch Jump"] }
    float [loop_beats] "loopBeats" "Slice" { None, Linear, 0.125, 16.0, 1.0 }
    float [amount] "amount" "Amount" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [pitch_semitones] "pitchSemitones" "Pitch Jump" { Semitones, Linear, -24.0, 24.0, 12.0 }
    float [feedback] "feedback" "Feedback" { Fraction, Linear, 0.0, 0.95, 0.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 1.0 }
    toggle [xy_enabled] "xyEnabled" "XY control" { false }
    float [x] "x" "X" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [y] "y" "Y" { Fraction, Linear, 0.0, 1.0, 0.5 }
});
