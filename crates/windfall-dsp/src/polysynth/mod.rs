//! Windfall's subtractive, hybrid and matrix instruments.
//!
//! Each one is its own instrument with its own settings and synthesis graph
//! and tests; the fixed voice memory is shared by typed runtime instances.
//! Shared core blocks provide glide, smoothing and filters; effects provide
//! chorus, delay, phaser and reverb. Voices live in fixed arrays,
//! and every table a patch needs is built in `prepare`, so nothing here
//! allocates, locks or waits on the audio thread.
//!
//! `docs/qa/2026-10-10-inst-subtractive-fm.md` maps the instruments to the
//! parity rows they stand for and lists what each one's tests measure.

mod compact_synth;
mod core;
mod dual_poly;
mod fx;
mod groove_synth;
mod hybrid_seq;
mod matrix_synth;
mod operators;
mod pack_assets;
mod pack_synth;
mod pocket_synth;
mod runtime;
mod vintage_poly;
mod vintage_saw;
pub use compact_synth::{CompactResponse, CompactSynth, CompactSynthParams};
pub use dual_poly::{DualPoly, DualPolyParams, PolyResponse};
pub use groove_synth::{GrooveMode, GrooveSynth, GrooveSynthParams};
pub use hybrid_seq::{HybridAlgorithm, HybridSeq, HybridSeqParams, HybridStep};
pub use matrix_synth::{MatrixSynth, MatrixSynthParams};
pub use operators::{MatrixOperator, OperatorWave};
pub use pack_synth::{PackPatch, PackSynth, PackSynthParams};
pub use pocket_synth::{PocketSynth, PocketSynthParams};
pub use vintage_poly::{VintagePoly, VintagePolyParams};
pub use vintage_saw::{VintageSaw, VintageSawParams};

#[cfg(test)]
mod tests;

pub use core::MAX_VOICES;
