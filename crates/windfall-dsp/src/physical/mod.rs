//! Bounded physical string models. Construction/prepare own all allocation;
//! note events, reset, control changes and rendering only touch fixed storage.

mod engine;
mod params;

pub use params::{AcousticStringParams, FingerBassParams, PluckParams};

use crate::instrument::Instrument;
use crate::param::ParamSet;
use engine::{Engine, Model, Settings};

/// Maximum simultaneous strings, including released voices.
pub const MAX_POLYPHONY: usize = 8;

macro_rules! instrument {
    ($name:ident, $params:ident, $model:ident) => {
        pub struct $name {
            engine: Engine,
            params: $params,
            current: $params,
            moving: bool,
        }

        impl Default for $name {
            fn default() -> Self {
                Self {
                    engine: Engine::new(Model::$model),
                    params: $params::default(),
                    current: $params::default(),
                    moving: false,
                }
            }
        }

        impl Instrument for $name {
            type Params = $params;

            fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
                self.engine.prepare(sample_rate);
                self.reset();
            }

            fn reset(&mut self) {
                self.current = self.params;
                self.moving = false;
                self.engine.reset();
            }

            fn set_params(&mut self, params: &$params) {
                self.params = params.sanitized();
                if self.engine.fresh {
                    self.current = self.params;
                    self.moving = false;
                } else {
                    self.moving = true;
                }
            }

            fn note_on(&mut self, key: u8, velocity: f32) {
                if !velocity.is_finite() || velocity <= 0.0 {
                    self.note_off(key);
                    return;
                }
                self.engine
                    .note_on(key.min(127), velocity.min(1.0), self.current.settings());
            }

            fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
                self.engine.set_note_pan_law(law);
            }
            fn note_on_expression(
                &mut self,
                key: u8,
                velocity: f32,
                pan: f32,
                _expression: crate::NoteExpression,
            ) {
                self.note_on(key, velocity);
                if velocity.is_finite() && velocity > 0.0 {
                    self.engine.pan_latest(pan);
                }
            }
            fn note_off(&mut self, key: u8) {
                self.engine.note_off(key.min(127));
            }

            fn all_notes_off(&mut self) {
                self.engine.all_notes_off();
            }

            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                left.fill(0.0);
                right.fill(0.0);
                for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                    self.engine.fresh = false;
                    if self.engine.until_control == 0 {
                        if self.moving {
                            self.moving =
                                self.current.approach(&self.params, self.engine.smoothing);
                        }
                        self.engine.control(self.current.settings());
                        self.engine.until_control = crate::blocks::CONTROL_PERIOD;
                    }
                    self.engine.until_control -= 1;
                    let sample = self.engine.tick(self.current.settings());
                    *l = sample.0;
                    *r = sample.1;
                }
            }

            fn active_voices(&self) -> usize {
                self.engine.active_voices()
            }
        }
    };
}

instrument!(Pluck, PluckParams, Pluck);
instrument!(FingerBass, FingerBassParams, FingerBass);
instrument!(AcousticString, AcousticStringParams, AcousticString);

impl PluckParams {
    fn settings(self) -> Settings {
        Settings {
            decay: self.decay_seconds,
            brightness: self.brightness,
            pick: self.pick_position,
            damping: 0.0,
            body: 0.0,
            stiffness: 0.0,
            sympathetic: 0.0,
            level: self.level,
        }
    }
}

impl FingerBassParams {
    fn settings(self) -> Settings {
        Settings {
            decay: self.decay_seconds,
            brightness: self.tone,
            pick: self.pick_position,
            damping: self.damping,
            body: self.body,
            stiffness: 0.0,
            sympathetic: 0.0,
            level: self.level,
        }
    }
}

impl AcousticStringParams {
    fn settings(self) -> Settings {
        Settings {
            decay: self.decay_seconds,
            brightness: self.brightness,
            pick: self.pick_position,
            damping: 0.0,
            body: 0.0,
            stiffness: self.stiffness,
            sympathetic: self.sympathetic,
            level: self.level,
        }
    }
}

#[cfg(test)]
mod tests;
