//! Additive and resynthesis instruments: six ways of building a sound out
//! of its partials instead of filtering one down to shape.
//!
//! # What they share
//!
//! Five of the six are banks of sine partials. Each note is up to 64 sines
//! at its own frequency and level, added together; the instruments differ
//! only in where those frequencies and levels come from. The sixth,
//! [`ScanSynth`], is not additive at all: a row of values is its waveform.
//! It is here because it belongs to the same family of instruments that are
//! edited as data rather than as controls, and because it shares the engine
//! underneath.
//!
//! Everything below the spectrum is the same for all six and lives in
//! [`VoicingParams`]: one envelope per note, a bounded number of notes at
//! once, how hard they are played and where the result sits. None of them
//! has a filter, an LFO or a modulation matrix. What the partials say is
//! what is heard.
//!
//! # Which is which
//!
//! | Instrument | Where the spectrum comes from | Parity |
//! |---|---|---|
//! | [`HarmonicStack`] | a level per harmonic, set by hand | `inst-harmless` |
//! | [`PartialMorph`] | two spectra and a control between them | `inst-morphine` |
//! | [`Inharmonic`] | a stiff-string model, so no partial is a harmonic | `inst-ogun` |
//! | [`Resynth`] | a table of ratio, level and phase | `inst-harmor` |
//! | [`SeedPatch`] | one number, through a random generator | `inst-autogun` |
//! | [`ScanSynth`] | a 32-value row read as the waveform | `inst-beepmap` |
//!
//! # Limits
//!
//! - 64 partials per note, 16 notes at once, both fixed at compile time.
//!   An additive voice costs one sine per partial per sample, so the
//!   polyphony control is what decides what an instrument costs.
//! - Partial levels add up. There is no automatic normalising: the volume
//!   control is where room for a chord is made.
//! - Notes are keys and velocities. Per-note expression, pitch bend and
//!   note instances are not implemented, so the trait's defaults apply.
//! - [`Resynth`] plays a table that is already in the project. Reading an
//!   audio file or an image to fill it is not included.
//! - [`ScanSynth`] is not band-limited, by design.
//!
//! The seam notes in `docs/integration/seams/additive.md` say what is left
//! for the parent to wire up.

mod engine;
mod harmonic_stack;
mod inharmonic;
mod labels;
mod partial_morph;
mod partials;
mod resynth;
mod rows;
mod scan;
mod scan_synth;
mod seed_patch;
mod voicing;

#[cfg(test)]
mod tests;

pub use engine::{GLIDE_MS, MAX_PARTIALS, MAX_POLYPHONY};
pub use harmonic_stack::{HarmonicStack, HarmonicStackParams, PartialGains};
pub use inharmonic::{Inharmonic, InharmonicParams};
pub use partial_morph::{MORPH_PARTIALS, PartialMorph, PartialMorphParams};
pub use resynth::{Resynth, ResynthParams, TABLE_PARTIALS, TablePartial};
pub use scan::SCAN_STEPS;
pub use scan_synth::{ScanSynth, ScanSynthParams};
pub use seed_patch::{MAX_SEED, SeedPatch, SeedPatchParams};
pub use voicing::VoicingParams;

mod prepared;
mod snapshot_morph;
pub use snapshot_morph::{SnapshotMorph,SnapshotMorphParams};

#[cfg(test)]
mod prepared_tests;
