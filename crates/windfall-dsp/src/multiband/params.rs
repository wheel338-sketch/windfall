use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BandCount {
    Two,
    #[default]
    Three,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct BandSplitParams {
    pub bands: BandCount,
    /// Hz; the sole split point in two-band mode.
    pub low_crossover_hz: f32,
    /// Hz; internally constrained above the lower edge and below Nyquist.
    pub high_crossover_hz: f32,
    pub low_gain_db: f32,
    /// Ignored in two-band mode; upper bands both use high_gain_db.
    pub mid_gain_db: f32,
    pub high_gain_db: f32,
}
impl Default for BandSplitParams {
    fn default() -> Self {
        Self {
            bands: BandCount::Three,
            low_crossover_hz: 200.0,
            high_crossover_hz: 2_500.0,
            low_gain_db: 0.0,
            mid_gain_db: 0.0,
            high_gain_db: 0.0,
        }
    }
}
param_set!(BandSplitParams, "Band Split", {
    choice [bands] "bands" "Bands" { BandCount, Three, [Two "two" "Two", Three "three" "Three"] }
    float [low_crossover_hz] "lowCrossoverHz" "Low crossover" { Hertz, Logarithmic, 20.0, 2_000.0, 200.0 }
    float [high_crossover_hz] "highCrossoverHz" "High crossover" { Hertz, Logarithmic, 200.0, 18_000.0, 2_500.0 }
    float [low_gain_db] "lowGainDb" "Low gain" { Decibels, Linear, -60.0, 24.0, 0.0 }
    float [mid_gain_db] "midGainDb" "Mid gain" { Decibels, Linear, -60.0, 24.0, 0.0 }
    float [high_gain_db] "highGainDb" "High gain" { Decibels, Linear, -60.0, 24.0, 0.0 }
});

/// Independent stereo-linked peak dynamics settings for one band.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct BandDynamicsParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub makeup_db: f32,
}
impl Default for BandDynamicsParams {
    fn default() -> Self {
        Self {
            threshold_db: -18.0,
            ratio: 4.0,
            attack_ms: 10.0,
            release_ms: 120.0,
            makeup_db: 0.0,
        }
    }
}
param_set!(BandDynamicsParams, "Band Dynamics", {
    float [threshold_db] "thresholdDb" "Threshold" { Decibels, Linear, -60.0, 0.0, -18.0 }
    float [ratio] "ratio" "Ratio" { Ratio, Logarithmic, 1.0, 100.0, 4.0 }
    float [attack_ms] "attackMs" "Attack" { Milliseconds, Logarithmic, 0.05, 250.0, 10.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 5.0, 2_500.0, 120.0 }
    float [makeup_db] "makeupDb" "Makeup" { Decibels, Linear, -24.0, 24.0, 0.0 }
});

macro_rules! dynamics_params {
    ($ty:ident, $name:literal, { $($extra:tt)* }) => {
        param_set!($ty, $name, {
            float [low_crossover_hz] "lowCrossoverHz" "Low crossover" { Hertz, Logarithmic, 20.0, 2_000.0, 200.0 }
            float [high_crossover_hz] "highCrossoverHz" "High crossover" { Hertz, Logarithmic, 200.0, 18_000.0, 2_500.0 }
            float [low.threshold_db] "low.thresholdDb" "Low threshold" { Decibels, Linear, -60.0, 0.0, -18.0 }
            float [low.ratio] "low.ratio" "Low ratio" { Ratio, Logarithmic, 1.0, 100.0, 4.0 }
            float [low.attack_ms] "low.attackMs" "Low attack" { Milliseconds, Logarithmic, 0.05, 250.0, 10.0 }
            float [low.release_ms] "low.releaseMs" "Low release" { Milliseconds, Logarithmic, 5.0, 2_500.0, 120.0 }
            float [low.makeup_db] "low.makeupDb" "Low makeup" { Decibels, Linear, -24.0, 24.0, 0.0 }
            float [mid.threshold_db] "mid.thresholdDb" "Mid threshold" { Decibels, Linear, -60.0, 0.0, -18.0 }
            float [mid.ratio] "mid.ratio" "Mid ratio" { Ratio, Logarithmic, 1.0, 100.0, 4.0 }
            float [mid.attack_ms] "mid.attackMs" "Mid attack" { Milliseconds, Logarithmic, 0.05, 250.0, 10.0 }
            float [mid.release_ms] "mid.releaseMs" "Mid release" { Milliseconds, Logarithmic, 5.0, 2_500.0, 120.0 }
            float [mid.makeup_db] "mid.makeupDb" "Mid makeup" { Decibels, Linear, -24.0, 24.0, 0.0 }
            float [high.threshold_db] "high.thresholdDb" "High threshold" { Decibels, Linear, -60.0, 0.0, -18.0 }
            float [high.ratio] "high.ratio" "High ratio" { Ratio, Logarithmic, 1.0, 100.0, 4.0 }
            float [high.attack_ms] "high.attackMs" "High attack" { Milliseconds, Logarithmic, 0.05, 250.0, 10.0 }
            float [high.release_ms] "high.releaseMs" "High release" { Milliseconds, Logarithmic, 5.0, 2_500.0, 120.0 }
            float [high.makeup_db] "high.makeupDb" "High makeup" { Decibels, Linear, -24.0, 24.0, 0.0 }
            toggle [sidechain] "sidechain" "External sidechain" { false }
            $($extra)*
        });
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MultibandCompressorParams {
    pub low_crossover_hz: f32,
    pub high_crossover_hz: f32,
    pub low: BandDynamicsParams,
    pub mid: BandDynamicsParams,
    pub high: BandDynamicsParams,
    /// Detector-only key with its own matching crossover. Missing key is silence.
    pub sidechain: bool,
}
impl Default for MultibandCompressorParams {
    fn default() -> Self {
        Self {
            low_crossover_hz: 200.0,
            high_crossover_hz: 2_500.0,
            low: BandDynamicsParams::default(),
            mid: BandDynamicsParams::default(),
            high: BandDynamicsParams::default(),
            sidechain: false,
        }
    }
}
dynamics_params!(MultibandCompressorParams, "Multiband Compressor", {});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MultibandMaximizerParams {
    pub low_crossover_hz: f32,
    pub high_crossover_hz: f32,
    pub low: BandDynamicsParams,
    pub mid: BandDynamicsParams,
    pub high: BandDynamicsParams,
    pub sidechain: bool,
    pub input_gain_db: f32,
    pub ceiling_db: f32,
    pub limiter_release_ms: f32,
    /// Independent sample peak ceilings, after each band's compression.
    pub low_ceiling_db: f32,
    pub mid_ceiling_db: f32,
    pub high_ceiling_db: f32,
}
impl Default for MultibandMaximizerParams {
    fn default() -> Self {
        Self {
            low_crossover_hz: 200.0,
            high_crossover_hz: 2_500.0,
            low: BandDynamicsParams::default(),
            mid: BandDynamicsParams::default(),
            high: BandDynamicsParams::default(),
            sidechain: false,
            input_gain_db: 0.0,
            ceiling_db: -0.3,
            limiter_release_ms: 100.0,
            low_ceiling_db: 0.0,
            mid_ceiling_db: 0.0,
            high_ceiling_db: 0.0,
        }
    }
}
dynamics_params!(MultibandMaximizerParams, "Multiband Maximizer", {
    float [input_gain_db] "inputGainDb" "Input gain" { Decibels, Linear, -12.0, 24.0, 0.0 }
    float [ceiling_db] "ceilingDb" "Ceiling" { Decibels, Linear, -24.0, 0.0, -0.3 }
    float [limiter_release_ms] "limiterReleaseMs" "Limiter release" { Milliseconds, Logarithmic, 1.0, 1_000.0, 100.0 }
    float [low_ceiling_db] "lowCeilingDb" "Low ceiling" { Decibels, Linear, -24.0, 0.0, 0.0 }
    float [mid_ceiling_db] "midCeilingDb" "Mid ceiling" { Decibels, Linear, -24.0, 0.0, 0.0 }
    float [high_ceiling_db] "highCeilingDb" "High ceiling" { Decibels, Linear, -24.0, 0.0, 0.0 }
});

// Small processors deliberately have different public control sets.
macro_rules! small_params {
    ($ty:ident, $name:literal, { $( $field:ident $id:literal $label:literal { $unit:ident, $scale:ident, $min:expr, $max:expr, $default:expr } )+ }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase", default)]
        #[ts(export)]
        pub struct $ty { $(pub $field: f32,)+ }
        impl Default for $ty { fn default() -> Self { Self { $($field: $default,)+ } } }
        param_set!($ty, $name, { $(float [$field] $id $label { $unit, $scale, $min, $max, $default })+ });
    };
}
small_params!(OneKnobParams, "One Knob", {
    amount "amount" "Amount" { Fraction, Linear, 0.0, 1.0, 0.0 }
});
/// Which part of the shaped sound a [`TransientShaperParams`] lets out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TransientListen {
    /// The whole shaped sound.
    #[default]
    Both,
    /// Only what the detector calls attack.
    Attack,
    /// Only the rest: the body and the release.
    Sustain,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct TransientShaperParams {
    /// -1 softens the attack by 18 dB, 1 boosts it by 18 dB; default 0.
    pub attack: f32,
    /// The same for everything that is not attack; default 0.
    pub sustain: f32,
    /// How readily a rise counts as an attack, 0.25 to 4; default 1.
    pub sensitivity: f32,
    /// The part that is put out. The two parts add up to the whole, so two
    /// instances on two tracks split a sound into its attack and its body.
    pub listen: TransientListen,
}
impl Default for TransientShaperParams {
    fn default() -> Self {
        Self {
            attack: 0.0,
            sustain: 0.0,
            sensitivity: 1.0,
            listen: TransientListen::Both,
        }
    }
}
// `listen` is appended: automation addresses the first three by index.
param_set!(TransientShaperParams, "Transient Shaper", {
    float [attack] "attack" "Attack" { None, Linear, -1.0, 1.0, 0.0 }
    float [sustain] "sustain" "Sustain" { None, Linear, -1.0, 1.0, 0.0 }
    float [sensitivity] "sensitivity" "Sensitivity" { Gain, Linear, 0.25, 4.0, 1.0 }
    choice [listen] "listen" "Output" {
        TransientListen, Both, [Both "both" "Both", Attack "attack" "Attack only", Sustain "sustain" "Sustain only"]
    }
});
small_params!(TransientSplitParams, "Transient Split", {
    transient_gain_db "transientGainDb" "Transient gain" { Decibels, Linear, -60.0, 24.0, 0.0 }
    sustain_gain_db "sustainGainDb" "Sustain gain" { Decibels, Linear, -60.0, 24.0, 0.0 }
    sensitivity "sensitivity" "Sensitivity" { Gain, Linear, 0.25, 4.0, 1.0 }
});
small_params!(BassHarmonicsParams, "Bass Harmonics", {
    cutoff_hz "cutoffHz" "Bass cutoff" { Hertz, Logarithmic, 40.0, 500.0, 180.0 }
    drive "drive" "Drive" { Gain, Logarithmic, 1.0, 12.0, 3.0 }
    amount "amount" "Amount" { Fraction, Linear, 0.0, 1.0, 0.0 }
});
small_params!(ExciterParams, "Exciter", {
    frequency_hz "frequencyHz" "Excitation frequency" { Hertz, Logarithmic, 500.0, 12_000.0, 3_000.0 }
    drive "drive" "Drive" { Gain, Logarithmic, 1.0, 8.0, 2.0 }
    mix "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
    color "color" "Color" { None, Linear, -1.0, 1.0, 0.0 }
});
