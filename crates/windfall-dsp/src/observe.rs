//! What an effect measured while it processed its last block.
//!
//! An observation is a plain `Copy` value that the effect leaves in its own
//! fields. The thread that ran `process` reads it straight afterwards and
//! carries it to wherever a display wants it. Reading does not reset
//! anything, so it never takes a reading away from the effect's gain
//! reduction meter, and nothing here is shared between threads.

use crate::tuner::TunerReadout;

/// The deepest attenuation a closed gate is reported as, in dB.
pub const GATE_FLOOR_DB: f32 = 120.0;

/// Levels a dynamics processor saw over one processed block.
///
/// The peaks are linear sample peaks of the louder channel. The input peak
/// is taken after the input gain and before the gate, the compression and
/// the limiting; the output peak is what left the effect. A processor with
/// look-ahead puts its output out late, so the two peaks of one block are
/// not the same moment of the signal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DynamicsObservation {
    /// Counts processed blocks, so a reader can tell a new observation from
    /// one it has already seen. An effect that is switched off stops
    /// counting.
    pub serial: u64,
    /// Frames in the block.
    pub frames: u32,
    pub input_peak: f32,
    pub output_peak: f32,
    /// The deepest compression plus limiting in the block, in dB, 0 or more.
    pub reduction_db: f32,
    /// The deepest gate attenuation in the block, in dB, from 0 (open) to
    /// [`GATE_FLOOR_DB`] (shut).
    pub gate_db: f32,
}

/// The reading of one effect after one block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectObservation {
    Dynamics(DynamicsObservation),
    Tuner {
        /// Counts processed blocks, as in [`DynamicsObservation::serial`].
        serial: u64,
        readout: TunerReadout,
    },
}

impl EffectObservation {
    pub fn serial(&self) -> u64 {
        match self {
            EffectObservation::Dynamics(observation) => observation.serial,
            EffectObservation::Tuner { serial, .. } => *serial,
        }
    }
}
