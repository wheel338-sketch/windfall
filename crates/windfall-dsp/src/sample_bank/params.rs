use crate::param::param_set;
use crate::{ChorusParams, DelayParams, ReverbParams};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub(super) trait BankControls {
    fn sf2_controls(&self) -> Option<super::sf2_voice::Controls> { None }
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
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SoundFontParams {
    pub level: f32,
    pub tune: f32,
    pub release_ms: f32,
    pub brightness: f32,
    pub mod_wheel: i32,
    pub pressure: i32,
    pub pitch_wheel: i32,
    pub wheel_sensitivity: f32,
    pub volume: i32,
    pub pan: i32,
    pub expression: i32,
    pub reverb_send: i32,
    pub chorus_send: i32,
    pub controllers: [crate::midi_out::MidiOutController; 8],
}
impl Default for SoundFontParams {
    fn default() -> Self {
        Self { level:0.5, tune:0.0,release_ms:100.0,brightness:1.0,
            mod_wheel:0,pressure:0,pitch_wheel:8192,wheel_sensitivity:2.0,
            volume:127,pan:64,expression:127,reverb_send:0,chorus_send:0,
            controllers:[crate::midi_out::MidiOutController::default();8] }
    }
}
impl BankControls for SoundFontParams {
    fn sf2_controls(&self)->Option<super::sf2_voice::Controls> {
        let mut c=super::sf2_voice::Controls::default();
        c.cc[1]=self.mod_wheel as u8;c.cc[7]=self.volume as u8;
        c.cc[10]=self.pan as u8;c.cc[11]=self.expression as u8;
        c.cc[91]=self.reverb_send as u8;c.cc[93]=self.chorus_send as u8;
        c.pressure=self.pressure as u8;c.wheel=self.pitch_wheel as u16;
        c.sensitivity=self.wheel_sensitivity;
        for controller in self.controllers {
            if (0..128).contains(&controller.number) { c.cc[controller.number as usize]=controller.value as u8; }
        }
        Some(c)
    }
}
param_set!(SoundFontParams, "SoundFont bank", {
    float [level] "level" "Level" { None, Linear, 0.0, 2.0, 0.5 }
    float [tune] "tune" "Tune" { Semitones, Linear, -48.0, 48.0, 0.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds, Logarithmic, 1.0, 10000.0, 100.0 }
    float [brightness] "brightness" "Brightness" { None, Linear, 0.0, 1.0, 1.0 }
    int [mod_wheel] "modWheel" "Modulation wheel (CC1)" { None, 0, 127, 0 }
    int [pressure] "pressure" "Channel pressure" { None, 0, 127, 0 }
    int [pitch_wheel] "pitchWheel" "Pitch wheel" { None, 0, 16383, 8192 }
    float [wheel_sensitivity] "wheelSensitivity" "Pitch wheel sensitivity" { Semitones, Linear, 0.0, 127.0, 2.0 }
    int [volume] "volume" "Volume (CC7)" { None, 0, 127, 127 }
    int [pan] "pan" "Pan (CC10)" { None, 0, 127, 64 }
    int [expression] "expression" "Expression (CC11)" { None, 0, 127, 127 }
    int [reverb_send] "reverbSend" "Reverb send (CC91)" { None, 0, 127, 0 }
    int [chorus_send] "chorusSend" "Chorus send (CC93)" { None, 0, 127, 0 }
    int [controllers[0].number] "controllers.0.number" "Controller 1 number" { None, -1, 127, -1 }
    int [controllers[0].value] "controllers.0.value" "Controller 1 value" { None, 0, 127, 0 }
    int [controllers[1].number] "controllers.1.number" "Controller 2 number" { None, -1, 127, -1 }
    int [controllers[1].value] "controllers.1.value" "Controller 2 value" { None, 0, 127, 0 }
    int [controllers[2].number] "controllers.2.number" "Controller 3 number" { None, -1, 127, -1 }
    int [controllers[2].value] "controllers.2.value" "Controller 3 value" { None, 0, 127, 0 }
    int [controllers[3].number] "controllers.3.number" "Controller 4 number" { None, -1, 127, -1 }
    int [controllers[3].value] "controllers.3.value" "Controller 4 value" { None, 0, 127, 0 }
    int [controllers[4].number] "controllers.4.number" "Controller 5 number" { None, -1, 127, -1 }
    int [controllers[4].value] "controllers.4.value" "Controller 5 value" { None, 0, 127, 0 }
    int [controllers[5].number] "controllers.5.number" "Controller 6 number" { None, -1, 127, -1 }
    int [controllers[5].value] "controllers.5.value" "Controller 6 value" { None, 0, 127, 0 }
    int [controllers[6].number] "controllers.6.number" "Controller 7 number" { None, -1, 127, -1 }
    int [controllers[6].value] "controllers.6.value" "Controller 7 value" { None, 0, 127, 0 }
    int [controllers[7].number] "controllers.7.number" "Controller 8 number" { None, -1, 127, -1 }
    int [controllers[7].value] "controllers.7.value" "Controller 8 value" { None, 0, 127, 0 }
});
controls!(PadBankParams, "Layered pads");
controls!(PianoKeysParams, "Piano keys");
controls!(TextVoiceParams, "Text voice");
