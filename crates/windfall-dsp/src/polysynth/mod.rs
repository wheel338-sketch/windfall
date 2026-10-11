//! Windfall's subtractive, hybrid and matrix instruments.
//!
//! Each one is its own instrument with its own settings, voice structure
//! and tests; what they share is in [`core`] (voice allocation, glide,
//! smoothed controls, the ladder and saturating filters) and [`fx`] (the
//! built-in chorus, delay, phaser and reverb). Voices live in fixed arrays,
//! and every table a patch needs is built in `prepare`, so nothing here
//! allocates, locks or waits on the audio thread.
//!
//! `docs/qa/2026-10-10-inst-subtractive-fm.md` maps the instruments to the
//! parity rows they stand for and lists what each one's tests measure.

mod core;
mod fx;
mod runtime;
mod vintage_poly;
pub use vintage_poly::{VintagePoly, VintagePolyParams};

#[cfg(test)]
mod tests;

pub use core::MAX_VOICES;
