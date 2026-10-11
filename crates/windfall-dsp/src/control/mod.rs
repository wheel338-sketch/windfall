//! Bounded control sources and their audible stereo applications.
//!
//! Control blocks contain one output per sample, in caller-owned fixed arrays.
//! Calling a control block advances the same state as audio processing; a host
//! needing both should use `process_with_control` rather than advance twice.

mod envelope;
mod formula;
mod keyboard;
mod motion;
mod pads;
mod voltage;
pub use voltage::{VoltageSource, VoltageSourceParams};

pub use envelope::{EnvelopeFollower, EnvelopeFollowerParams, NoteEnvelope, NoteEnvelopeParams};
pub use formula::{FormulaError, FormulaSource, FormulaSourceParams};
pub use keyboard::{KeyboardControl, KeyboardSource, KeyboardSourceParams};
pub use motion::{PanLfo, PanLfoParams};
pub use pads::{XyPad, XyPadParams, XyzPad, XyzPadParams};

use crate::blocks::math::{clean, flush, ms_to_samples};
use crate::blocks::smooth::LinearRamp;

fn rate(value: f32) -> f32 {
    clean(value, 1.0, 384_000.0, 48_000.0)
}

fn audio(value: f32) -> f32 {
    flush(clean(value, -1.0e3, 1.0e3, 0.0))
}

fn peak(left: f32, right: f32) -> f32 {
    audio(left).abs().max(audio(right).abs()).min(1.0)
}

/// Unity at center; attenuate the opposite channel as balance moves.
fn balance(left: f32, right: f32, pan: f32, gain: f32) -> [f32; 2] {
    [
        audio(audio(left) * gain * (1.0 - pan.max(0.0))),
        audio(audio(right) * gain * (1.0 + pan.min(0.0))),
    ]
}

struct Controls<const N: usize> {
    ramps: [LinearRamp; N],
    samples: u32,
    fresh: bool,
}

impl<const N: usize> Controls<N> {
    fn new(values: [f32; N]) -> Self {
        Self {
            ramps: values.map(LinearRamp::new),
            samples: 240,
            fresh: true,
        }
    }

    fn prepare(&mut self, sample_rate: f32) {
        self.samples = ms_to_samples(5.0, rate(sample_rate));
        self.reset();
    }

    fn reset(&mut self) {
        for ramp in &mut self.ramps {
            ramp.snap(ramp.target());
        }
        self.fresh = true;
    }

    fn set(&mut self, values: [f32; N]) {
        for (ramp, value) in self.ramps.iter_mut().zip(values) {
            ramp.set_target(value, if self.fresh { 0 } else { self.samples });
        }
    }

    fn tick(&mut self) -> [f32; N] {
        self.fresh = false;
        std::array::from_fn(|i| self.ramps[i].tick())
    }
}

#[cfg(test)]
mod tests;
