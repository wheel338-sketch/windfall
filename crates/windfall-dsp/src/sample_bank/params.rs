use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

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
controls!(MultiSamplerParams, "Multisampler");
controls!(SamplePlayerParams, "Sample player");
controls!(SoundFontParams, "SoundFont bank");
controls!(PadBankParams, "Layered pads");
controls!(PianoKeysParams, "Piano keys");
controls!(TextVoiceParams, "Text voice");
