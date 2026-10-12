//! A unity mixer device. Its control-side worker consumes the existing meters.
use crate::param::param_set;
use crate::{Effect, ParamSet};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MusicLightingParams {
    /// 0 tempo pulse, 1 audio energy, 2 tempo multiplied by audio energy.
    pub mode: i32,
    /// 0 all, 1 keyboard, 2 mouse, 3 mousepad, 4 headset, 5 keypad, 6 Chroma Link.
    pub device: i32,
    pub level: f32,
    pub red: i32,
    pub green: i32,
    pub blue: i32,
}
impl Default for MusicLightingParams {
    fn default() -> Self {
        Self {
            mode: 2,
            device: 0,
            level: 1.0,
            red: 255,
            green: 64,
            blue: 0,
        }
    }
}
param_set!(MusicLightingParams, "Music lighting", {
    int [mode] "mode" "Source (tempo, audio, both)" { None, 0, 2, 2 }
    int [device] "device" "Device (all, keyboard, mouse, pad, headset, keypad, link)" { None, 0, 6, 0 }
    float [level] "level" "Brightness" { Fraction, Linear, 0.0, 1.0, 1.0 }
    int [red] "red" "Red" { None, 0, 255, 255 }
    int [green] "green" "Green" { None, 0, 255, 64 }
    int [blue] "blue" "Blue" { None, 0, 255, 0 }
});
impl MusicLightingParams {
    pub fn color(self, beat_phase: f32, energy: f32, playing: bool) -> [u8; 3] {
        let params = self.sanitized();
        let phase = if beat_phase.is_finite() {
            beat_phase.rem_euclid(1.0)
        } else {
            0.0
        };
        let pulse = if playing {
            0.15 + 0.85 * (0.5 + 0.5 * (std::f32::consts::TAU * phase).cos())
        } else {
            0.0
        };
        let energy = if energy.is_finite() {
            energy.clamp(0.0, 1.0).sqrt()
        } else {
            0.0
        };
        let intensity = params.level
            * match params.mode {
                0 => pulse,
                1 => energy,
                _ => pulse * energy,
            };
        [params.red, params.green, params.blue]
            .map(|value| (value as f32 * intensity).round() as u8)
    }
}
#[derive(Default)]
pub struct MusicLighting;
impl Effect for MusicLighting {
    type Params = MusicLightingParams;
    fn prepare(&mut self, _rate: f32, _block: usize) {}
    fn reset(&mut self) {}
    fn set_params(&mut self, _params: &MusicLightingParams) {}
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lighting_color_uses_measured_energy_and_tempo_with_safe_extremes() {
        let params = MusicLightingParams {
            mode: 1,
            red: 255,
            green: 0,
            blue: 0,
            ..Default::default()
        };
        assert_eq!(params.color(0.0, 0.25, true), [128, 0, 0]);
        assert_eq!(params.color(0.0, f32::NAN, true), [0; 3]);
        let params = MusicLightingParams { mode: 0, ..params };
        assert_eq!(params.color(0.0, 0.0, true), [255, 0, 0]);
        assert_eq!(params.color(0.5, 0.0, true), [38, 0, 0]);
        assert_eq!(params.color(0.0, 1.0, false), [0; 3]);
        let mut effect = MusicLighting::default();
        effect.prepare(48000.0, 64);
        effect.set_params(&params);
        let mut left = [-0.5, 0.25, 0.75];
        let mut right = [0.3, -0.2, 0.5];
        let original = (left, right);
        effect.process(&mut left, &mut right);
        assert_eq!((left, right), original);
    }
}
