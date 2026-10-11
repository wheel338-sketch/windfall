use super::prepared::{prepared_instrument, Controls};
use crate::instrument_asset::SpectralBank;
use crate::param::param_set;
use crate::ParamSet;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SnapshotMorphParams {
    pub morph: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub level: f32,
    pub polyphony: u8,
}
impl Default for SnapshotMorphParams {
    fn default() -> Self {
        Self {
            morph: 0.0,
            attack_ms: 2.0,
            release_ms: 200.0,
            level: 0.2,
            polyphony: 8,
        }
    }
}
param_set!(SnapshotMorphParams,"Snapshot Morph",{
    float [morph] "morph" "Morph" { Fraction,Linear,0.0,1.0,0.0 }
    float [attack_ms] "attackMs" "Attack" { Milliseconds,Logarithmic,0.1,2000.0,2.0 }
    float [release_ms] "releaseMs" "Release" { Milliseconds,Logarithmic,1.0,4000.0,200.0 }
    float [level] "level" "Level" { Gain,Linear,0.0,1.0,0.2 }
    int [polyphony] "polyphony" "Polyphony" { None,1,16,8 }
});
impl SnapshotMorphParams {
    fn controls(self) -> Controls {
        Controls {
            position: self.morph,
            gain: self.level,
            attack: self.attack_ms,
            release: self.release_ms,
            polyphony: usize::from(self.polyphony),
            ..Default::default()
        }
    }
}
prepared_instrument!(
    SnapshotMorph,
    SnapshotMorphParams,
    SpectralBank::harmonic_snapshots()
);
