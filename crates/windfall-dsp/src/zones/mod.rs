//! Fixed-storage multisample instruments and a deliberately small SF2 importer.
//! Construction, preparation, SF2 import and destruction belong off audio.
mod engine;
mod params;
mod soundfont;

use crate::instrument::Instrument;
use crate::param::ParamSet;
use crate::{NoteExpression, NoteInstanceId};
use engine::Engine;
pub use params::{
    CrossfadeAxis, KeyBedParams, LoopMode, PadSamplerParams, Sample, Zone, ZonePlayerParams,
    ZoneSamplerParams, ZoneTable, ZoneTableError,
};
pub use soundfont::{SoundFontError, parse_soundfont, parse_soundfont_preset};

pub const MAX_ZONES: usize = 32;
pub const MAX_SAMPLE_FRAMES: usize = 1024;
/// Layer voices, not MIDI notes: an overlapping note can consume multiple slots.
pub const MAX_POLYPHONY: usize = 32;
pub const PAD_COUNT: usize = 16;
pub const FIRST_PAD_KEY: u8 = 36;

macro_rules! instrument {
    ($name:ident, $params:ident) => {
        /// Bounded sample playback. `new` installs sources before playback begins.
        pub struct $name {
            engine: Engine,
            params: $params,
        }
        impl Default for $name {
            fn default() -> Self {
                Self::new(&$params::default())
            }
        }
        impl $name {
            /// Off-audio construction. In particular, this installs a locked player's bank.
            pub fn new(params: &$params) -> Self {
                Self {
                    engine: Engine::default(),
                    params: params.sanitized(),
                }
            }
            /// Current sanitized parameters, including the installed zone table.
            pub fn params(&self) -> &$params {
                &self.params
            }
        }
        impl Instrument for $name {
            type Params = $params;
            fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
                self.engine.prepare(sample_rate);
                self.reset();
            }
            fn reset(&mut self) {
                self.engine.reset(self.params.level, self.params.release_ms);
            }
            fn set_params(&mut self, params: &$params) {
                // Lock is checked before applying the next flag. Unlocking a sampler
                // therefore permits zone replacement on the following set_params call.
                let mut next = params.sanitized();
                if self.params.zones_locked {
                    next.zones = self.params.zones;
                }
                self.params = next;
                self.engine.set_controls(next.level, next.release_ms);
            }
            fn note_on(&mut self, key: u8, velocity: f32) {
                self.note_on_expression(key, velocity, 0.0, NoteExpression::default());
            }
            fn note_on_expression(
                &mut self,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: NoteExpression,
            ) {
                if !velocity.is_finite() || velocity <= 0.0 {
                    self.note_off(key);
                    return;
                }
                let id = self.engine.legacy_id();
                self.note_on_instance(id, key, velocity, pan, expression);
            }
            fn supports_note_instances(&self) -> bool {
                true
            }
            fn note_on_instance(
                &mut self,
                id: NoteInstanceId,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: NoteExpression,
            ) {
                if !velocity.is_finite() || velocity <= 0.0 {
                    self.note_off_instance(id, key);
                    return;
                }
                self.engine.note_on(
                    id,
                    key,
                    velocity,
                    pan,
                    expression,
                    &self.params.zones,
                    self.params.crossfade,
                );
            }
            fn note_off(&mut self, key: u8) {
                self.engine.release_key(key);
            }
            fn note_off_instance(&mut self, id: NoteInstanceId, _key: u8) {
                self.engine.release_instance(id);
            }
            fn set_note_expression(
                &mut self,
                id: NoteInstanceId,
                pan: f32,
                expression: NoteExpression,
            ) {
                self.engine.expression(id, pan, expression);
            }
            fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
                self.engine.pitch(id, pitch);
            }
            fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
                self.engine.gain(id, gain);
            }
            fn all_notes_off(&mut self) {
                self.engine.stop();
            }
            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                left.fill(0.0);
                right.fill(0.0);
                for (left, right) in left.iter_mut().zip(right.iter_mut()) {
                    (*left, *right) = self.engine.tick();
                }
            }
            fn active_voices(&self) -> usize {
                self.engine.active_voices()
            }
        }
    };
}
instrument!(ZoneSampler, ZoneSamplerParams);
instrument!(ZonePlayer, ZonePlayerParams);
instrument!(PadSampler, PadSamplerParams);
instrument!(KeyBed, KeyBedParams);

#[cfg(test)]
mod tests;
