use super::prepared::{prepared_instrument, Controls};
use crate::instrument_asset::SpectralBank;
use crate::param::param_set;
use crate::ParamSet;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct NumberVoiceParams {
    pub level: f32,
}
impl Default for NumberVoiceParams {
    fn default() -> Self {
        Self { level: 0.2 }
    }
}
param_set!(NumberVoiceParams,"Number Voice",{ float [level] "level" "Level" {Gain,Linear,0.0,1.0,0.2} });
impl NumberVoiceParams {
    fn controls(self) -> Controls {
        Controls {
            gain: self.level,
            ..Default::default()
        }
    }
}
prepared_instrument!(
    NumberVoice,
    NumberVoiceParams,
    SpectralBank::numbered(12345)
);
