use super::prepared::{prepared_instrument, Controls};
use crate::instrument_asset::SpectralBank;
use crate::param::param_set;
use crate::ParamSet;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum HarmonicShape {
    #[default]
    Saw,
    Square,
    Triangle,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct HarmonicFilterParams {
    pub shape: HarmonicShape,
    pub cutoff_hz: f32,
    pub resonance: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub level: f32,
    pub polyphony: u8,
}
impl Default for HarmonicFilterParams {
    fn default() -> Self {
        Self {
            shape: HarmonicShape::Saw,
            cutoff_hz: 5000.0,
            resonance: 0.0,
            attack_ms: 2.0,
            release_ms: 200.0,
            level: 0.2,
            polyphony: 8,
        }
    }
}
param_set!(HarmonicFilterParams,"Harmonic Filter",{
    choice [shape] "shape" "Waveform" { HarmonicShape,Saw,[Saw "saw" "Saw",Square "square" "Square",Triangle "triangle" "Triangle"] }
    float [cutoff_hz] "cutoffHz" "Cutoff" {Hertz,Logarithmic,20.0,20000.0,5000.0}
    float [resonance] "resonance" "Resonance" {Fraction,Linear,0.0,1.0,0.0}
    float [attack_ms] "attackMs" "Attack" {Milliseconds,Logarithmic,0.1,2000.0,2.0}
    float [release_ms] "releaseMs" "Release" {Milliseconds,Logarithmic,1.0,4000.0,200.0}
    float [level] "level" "Level" {Gain,Linear,0.0,1.0,0.2}
    int [polyphony] "polyphony" "Polyphony" {None,1,16,8}
});
impl HarmonicFilterParams {
    fn controls(self) -> Controls {
        Controls {
            shape: match self.shape {
                HarmonicShape::Saw => 0,
                HarmonicShape::Square => 1,
                HarmonicShape::Triangle => 2,
            },
            cutoff: self.cutoff_hz,
            resonance: self.resonance,
            gain: self.level,
            attack: self.attack_ms,
            release: self.release_ms,
            polyphony: usize::from(self.polyphony),
            ..Default::default()
        }
    }
}
prepared_instrument!(
    HarmonicFilter,
    HarmonicFilterParams,
    SpectralBank::harmonics()
);
