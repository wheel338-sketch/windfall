//! Bounded streaming spectral effects. Construction/prepare own all storage;
//! audio, parameter changes and reset use that storage without allocation.

mod convolver;
mod formants;
mod frequency;
mod pitch;
mod vocoder;

pub use convolver::{
    CONVOLUTION_PARTITION, Convolver, ConvolverParams, MAX_IMPULSE_SAMPLES, PreparedImpulse,
};
pub use frequency::{FREQUENCY_SHIFTER_LATENCY, FrequencyShifter, FrequencyShifterParams};
pub use pitch::{
    PITCH_SHIFT_LATENCY, PitchCorrect, PitchCorrectParams, PitchScale, PitchShift, PitchShiftMode,
    PitchShiftParams,
};
pub use vocoder::{Vocoder, VocoderParams};

use crate::blocks::math::{clean, flush};

fn audio(value: f32) -> f32 {
    flush(clean(value, -1_000.0, 1_000.0, 0.0))
}
fn output(value: f32) -> f32 {
    flush(clean(value, -1.0e6, 1.0e6, 0.0))
}
fn rate(value: f32) -> f32 {
    clean(value, 8_000.0, 384_000.0, 48_000.0)
}

#[cfg(test)]
mod tests;
