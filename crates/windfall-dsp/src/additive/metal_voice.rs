use super::prepared::{Controls, prepared_instrument};
use crate::ParamSet;
use crate::instrument_asset::SpectralBank;
use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MetalVoiceParams {
    pub stretch: f32,
    pub shimmer_hz: f32,
    pub shimmer_depth: f32,
    pub decay_seconds: f32,
    pub release_ms: f32,
    pub level: f32,
    pub polyphony: u8,
}
impl Default for MetalVoiceParams {
    fn default() -> Self {
        Self {
            stretch: 0.2,
            shimmer_hz: 3.7,
            shimmer_depth: 0.6,
            decay_seconds: 3.0,
            release_ms: 200.0,
            level: 0.2,
            polyphony: 8,
        }
    }
}
param_set!(MetalVoiceParams,"Metal Voice",{
    float [stretch] "stretch" "Partial Stretch" {Fraction,Linear,0.0,1.0,0.2}
    float [shimmer_hz] "shimmerHz" "Shimmer Rate" {Hertz,Linear,0.0,20.0,3.7}
    float [shimmer_depth] "shimmerDepth" "Shimmer Depth" {Fraction,Linear,0.0,1.0,0.6}
    float [decay_seconds] "decaySeconds" "Decay" {Seconds,Logarithmic,0.05,12.0,3.0}
    float [release_ms] "releaseMs" "Release" {Milliseconds,Logarithmic,1.0,4000.0,200.0}
    float [level] "level" "Level" {Gain,Linear,0.0,1.0,0.2}
    int [polyphony] "polyphony" "Polyphony" {None,1,16,8}
});
impl MetalVoiceParams {
    fn controls(self) -> Controls {
        Controls {
            stretch: self.stretch,
            shimmer: self.shimmer_hz,
            shimmer_depth: self.shimmer_depth,
            decay: self.decay_seconds,
            release: self.release_ms,
            gain: self.level,
            polyphony: usize::from(self.polyphony),
            attack: 0.5,
            ..Default::default()
        }
    }
}
prepared_instrument!(MetalVoice, MetalVoiceParams, SpectralBank::metallic());
