use crate::param::param_set;
use crate::{ChorusParams, DelayParams, ReverbParams};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub(super) trait BankControls {
    fn internal_effects(&self) -> Option<(ChorusParams, DelayParams, ReverbParams)> {
        None
    }
}

macro_rules! controls {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
        #[serde(rename_all = "camelCase", default)]
        #[ts(export)]
        pub struct $name {
            pub level: f32,
            pub tune: f32,
            pub release_ms: f32,
            pub brightness: f32,
        }
        impl BankControls for $name {}
        impl Default for $name {
            fn default() -> Self {
                Self { level: 0.5, tune: 0.0, release_ms: 100.0, brightness: 1.0 }
            }
        }
        param_set!($name, $label, {
            float [level] "level" "Level" { None, Linear, 0.0, 2.0, 0.5 }
            float [tune] "tune" "Tune" { Semitones, Linear, -48.0, 48.0, 0.0 }
            float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 1.0, 10000.0, 100.0 }
            float [brightness] "brightness" "Brightness" { None, Linear, 0.0, 1.0, 1.0 }
        });
    };
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MultiSamplerParams {
    pub level: f32,
    pub tune: f32,
    pub release_ms: f32,
    pub brightness: f32,
    pub chorus: ChorusParams,
    pub delay: DelayParams,
    pub reverb: ReverbParams,
}
impl Default for MultiSamplerParams {
    fn default() -> Self {
        Self {
            level: 0.5,
            tune: 0.0,
            release_ms: 100.0,
            brightness: 1.0,
            chorus: ChorusParams {
                mix: 0.0,
                ..Default::default()
            },
            delay: DelayParams {
                mix: 0.0,
                ..Default::default()
            },
            reverb: ReverbParams {
                mix: 0.0,
                ..Default::default()
            },
        }
    }
}
impl BankControls for MultiSamplerParams {
    fn internal_effects(&self) -> Option<(ChorusParams, DelayParams, ReverbParams)> {
        Some((self.chorus, self.delay, self.reverb))
    }
}
param_set!(MultiSamplerParams, "Multisampler", {
    float [level] "level" "Level" { None, Linear, 0.0, 2.0, 0.5 }
    float [tune] "tune" "Tune" { Semitones, Linear, -48.0, 48.0, 0.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 1.0, 10000.0, 100.0 }
    float [brightness] "brightness" "Brightness" { None, Linear, 0.0, 1.0, 1.0 }
    float [chorus.mix] "chorus.mix" "Chorus mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [chorus.depth_ms] "chorus.depthMs" "Chorus depth" { Milliseconds, Linear, 0.0, 10.0, 4.0 }
    float [chorus.rate1_hz] "chorus.rate1Hz" "Chorus voice 1 rate" { Hertz, Linear, 0.0, 5.0, 0.6 }
    float [chorus.rate2_hz] "chorus.rate2Hz" "Chorus voice 2 rate" { Hertz, Linear, 0.0, 5.0, 0.8 }
    float [chorus.rate3_hz] "chorus.rate3Hz" "Chorus voice 3 rate" { Hertz, Linear, 0.0, 5.0, 1.1 }
    float [delay.mix] "delay.mix" "Delay mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
    toggle [delay.sync] "delay.sync" "Delay sync" { true }
    float [delay.time_ms] "delay.timeMs" "Delay time" { Milliseconds, Linear, 1.0, 2000.0, 250.0 }
    float [delay.feedback] "delay.feedback" "Delay feedback" { Fraction, Linear, 0.0, 0.95, 0.35 }
    float [reverb.mix] "reverb.mix" "Reverb mix" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [reverb.decay_s] "reverb.decayS" "Reverb decay" { Seconds, Logarithmic, 0.1, 20.0, 1.8 }
    float [reverb.damping] "reverb.damping" "Reverb damping" { Fraction, Linear, 0.0, 1.0, 0.5 }
    float [reverb.pre_delay_ms] "reverb.preDelayMs" "Reverb pre-delay" { Milliseconds, Linear, 0.0, 250.0, 10.0 }
});
controls!(SamplePlayerParams, "Sample player");
controls!(SoundFontParams, "SoundFont bank");
controls!(PadBankParams, "Layered pads");
controls!(PianoKeysParams, "Piano keys");
controls!(TextVoiceParams, "Text voice");
