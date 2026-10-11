use super::prepared::{Controls, prepared_instrument};
use crate::ParamSet;
use crate::instrument_asset::SpectralBank;
use crate::param::param_set;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ColorScanParams {
    pub position: f32,
    pub stereo_width: f32,
    pub scan: bool,
    pub scan_rate: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub level: f32,
    pub polyphony: u8,
}
impl Default for ColorScanParams {
    fn default() -> Self {
        Self {
            position: 0.0,
            stereo_width: 1.0,
            scan: true,
            scan_rate: 1.0,
            attack_ms: 2.0,
            release_ms: 200.0,
            level: 0.2,
            polyphony: 8,
        }
    }
}
param_set!(ColorScanParams,"Color Scan",{
 float [position] "position" "Frame Position" {Fraction,Linear,0.0,1.0,0.0}
 toggle [scan] "scan" "Scan Frames" {true}
 float [scan_rate] "scanRate" "Scan Speed" {Ratio,Logarithmic,0.05,8.0,1.0}
 float [attack_ms] "attackMs" "Attack" {Milliseconds,Logarithmic,0.1,2000.0,2.0}
 float [release_ms] "releaseMs" "Release" {Milliseconds,Logarithmic,1.0,4000.0,200.0}
 float [level] "level" "Level" {Gain,Linear,0.0,1.0,0.2}
 int [polyphony] "polyphony" "Polyphony" {None,1,16,8}
 float [stereo_width] "stereoWidth" "Color Stereo Width" {Fraction,Linear,0.0,1.0,1.0}
});
impl ColorScanParams {
    fn controls(self) -> Controls {
        Controls {
            position: self.position,
            pan_width: self.stereo_width,
            scan: self.scan,
            scan_rate: self.scan_rate,
            attack: self.attack_ms,
            release: self.release_ms,
            gain: self.level,
            polyphony: usize::from(self.polyphony),
            ..Default::default()
        }
    }
}
prepared_instrument!(ColorScan, ColorScanParams, SpectralBank::harmonics());
