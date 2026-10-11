use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::math::clean;
use crate::param::{ParamSet, param_set};

use super::Controls;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct KeyboardSourceParams {
    pub root_key: u8,
    pub pitch_amount: f32,
    pub transpose: f32,
    pub velocity_amount: f32,
}

impl Default for KeyboardSourceParams {
    fn default() -> Self {
        Self {
            root_key: 60,
            pitch_amount: 1.0,
            transpose: 0.0,
            velocity_amount: 1.0,
        }
    }
}

param_set!(KeyboardSourceParams, "Keyboard Source", {
    int [root_key] "rootKey" "Root Key" { None, 0, 127, 60 }
    float [pitch_amount] "pitchAmount" "Pitch Amount" { Ratio, Linear, 0.0, 2.0, 1.0 }
    float [transpose] "transpose" "Transpose" { Semitones, Linear, -48.0, 48.0, 0.0 }
    float [velocity_amount] "velocityAmount" "Velocity Amount" { Fraction, Linear, 0.0, 1.0, 1.0 }
});

/// Add `pitch_semitones` to the destination's pitch and `gain_offset` to
/// unity gain. Pitch is -302..302 semitones; gain offset is -1..0.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyboardControl {
    pub pitch_semitones: f32,
    pub gain_offset: f32,
}

/// Monophonic last-note control source; it does not transpose audio samples.
pub struct KeyboardSource {
    params: KeyboardSourceParams,
    key: u8,
    velocity: f32,
    controls: Controls<2>,
}

impl Default for KeyboardSource {
    fn default() -> Self {
        Self {
            params: KeyboardSourceParams::default(),
            key: 60,
            velocity: 0.0,
            controls: Controls::new([0.0, -1.0]),
        }
    }
}

impl KeyboardSource {
    pub fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.controls.prepare(sample_rate);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.key = self.params.root_key;
        self.velocity = 0.0;
        self.controls.reset();
        self.update();
    }

    fn update(&mut self) {
        self.controls.set([
            (f32::from(self.key) - f32::from(self.params.root_key)) * self.params.pitch_amount
                + self.params.transpose,
            self.params.velocity_amount * (self.velocity - 1.0),
        ]);
    }

    pub fn set_params(&mut self, params: &KeyboardSourceParams) {
        self.params = params.sanitized();
        self.update();
    }

    /// `key` is MIDI 0..127; `velocity` is normalized 0..1 (zero releases).
    /// Pass the held key with zero velocity to release. Changes glide over 5 ms.
    pub fn set_note(&mut self, key: u8, velocity: f32) {
        self.key = key.min(127);
        self.velocity = clean(velocity, 0.0, 1.0, 0.0);
        self.update();
    }

    pub fn process_control<const N: usize>(&mut self, output: &mut [KeyboardControl; N]) {
        for frame in output {
            let [pitch_semitones, gain_offset] = self.controls.tick();
            *frame = KeyboardControl {
                pitch_semitones,
                gain_offset,
            };
        }
    }
}

impl crate::Effect for KeyboardSource {
    type Params = KeyboardSourceParams;
    fn prepare(&mut self, rate: f32, block: usize) { KeyboardSource::prepare(self, rate, block); }
    fn reset(&mut self) { KeyboardSource::reset(self); }
    fn set_params(&mut self, params: &Self::Params) { KeyboardSource::set_params(self, params); }
    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}
