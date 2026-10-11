//! Deterministic, bounded drum synthesis, with no sample assets.
//!
//! [`Membrane`] is a single tunable drum, [`DrumRack`] has sixteen independent
//! synthesized pads, [`Kick`] emphasizes a fast pitch sweep, and [`DrumVoice`]
//! selects among four distinct percussion recipes. All callback state is inline.

mod engine;
mod modal;
mod layer_kick;
pub use layer_kick::{LayerKick, LayerKickParams};
mod params;

pub use params::{
    DrumMode, DrumPadParams, DrumRackParams, DrumVoiceParams, KickParams, MembraneParams,
};

use crate::instrument::Instrument;
use engine::{Engine, Recipe};

/// A tunable eight-mode membrane and coupled cavity with a noisy strike. MIDI 60 is its base pitch.
#[derive(Default)]
pub struct Membrane(Engine<MembraneParams, 1>);

/// Sixteen synthesized pads on MIDI keys 36 through 51, one voice per pad.
#[derive(Default)]
pub struct DrumRack(Engine<DrumRackParams, 16>);

/// A punchy swept sine and a separately decaying click. MIDI 36 is its base pitch.
#[derive(Default)]
pub struct Kick(Engine<KickParams, 8>);

/// Eight voices of kick, snare, hat or tom synthesis. MIDI 36 is the base pitch.
#[derive(Default)]
pub struct DrumVoice(Engine<DrumVoiceParams, 8>);

macro_rules! instrument {
    ($ty:ident, $params:ident, $voices:expr, $recipe:expr, $rack:expr) => {
        impl $ty {
            /// Maximum simultaneously sounding voices, including released voices.
            pub const MAX_VOICES: usize = $voices;
            pub const LATENCY_SAMPLES: usize = 0;
        }

        impl Instrument for $ty {
            type Params = $params;

            fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
                self.0.prepare(sample_rate);
            }
            fn reset(&mut self) {
                self.0.reset();
            }
            fn set_params(&mut self, params: &Self::Params) {
                self.0.set_params(params);
            }
            fn note_on(&mut self, key: u8, velocity: f32) {
                self.0.note_on(key, velocity, $recipe, $rack);
            }
            fn note_off(&mut self, key: u8) {
                self.0.note_off(key);
            }
            fn all_notes_off(&mut self) {
                self.0.all_notes_off();
            }
            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                self.0.process(left, right);
            }
            fn active_voices(&self) -> usize {
                self.0.active_voices()
            }
        }
    };
}

instrument!(Membrane, MembraneParams, 1, Recipe::Membrane, false);
instrument!(DrumRack, DrumRackParams, 16, Recipe::Membrane, true);
instrument!(Kick, KickParams, 8, Recipe::Kick, false);
instrument!(DrumVoice, DrumVoiceParams, 8, Recipe::Selected, false);

#[cfg(test)]
mod tests;

impl DrumVoice {
    /// Control-side preparation only; never replace this owner on audio.
    pub(crate) fn set_key_patches(&mut self, patches: &[crate::DrumKeyPatch]) {
        self.0.set_key_patches(patches);
    }
}
