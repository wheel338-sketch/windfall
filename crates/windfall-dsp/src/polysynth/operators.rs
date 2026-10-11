//! Bounded six-operator phase modulation. Feedback/cyclic edges use the previous
//! substep, independent of graph traversal order. No heap graph is copied to audio.
use super::{
    core,
    runtime::{self, Context, Tone},
};
use crate::{EnvelopeParams, Waveform};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperatorWave {
    Sine,
    Triangle,
    Saw,
    Square,
}
impl OperatorWave {
    fn waveform(self) -> Waveform {
        match self {
            Self::Sine => Waveform::Sine,
            Self::Triangle => Waveform::Triangle,
            Self::Saw => Waveform::Saw,
            Self::Square => Waveform::Square,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MatrixOperator {
    pub ratio: f32,
    pub detune_cents: f32,
    pub level: f32,
    pub feedback: f32,
    pub key_scale: f32,
    pub waveform: OperatorWave,
    pub envelope: EnvelopeParams,
}
impl Default for MatrixOperator {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            detune_cents: 0.0,
            level: 0.5,
            feedback: 0.0,
            key_scale: 0.0,
            waveform: OperatorWave::Sine,
            envelope: EnvelopeParams::default(),
        }
    }
}
pub(super) fn configure(operators: &[MatrixOperator; 6], tone: &mut Tone, rate: f32) {
    for (op, env) in operators.iter().zip(&mut tone.operators) {
        runtime::envelope(env, op.envelope, rate);
    }
}
#[allow(clippy::too_many_arguments)] // One fixed graph and one unison lane, no callback graph object.
pub(super) fn matrix(
    t: &mut Tone,
    c: &Context,
    ops: &[MatrixOperator; 6],
    fm: &[[f32; 6]; 6],
    ring: &[[f32; 6]; 6],
    u: usize,
    pitch: f32,
    lfo_index: f32,
) -> [f32; 6] {
    let previous = t.previous[u];
    let mut now = [0.0; 6];
    for (to, op) in ops.iter().enumerate() {
        let inc = core::increment(
            pitch + 12.0 * op.ratio.log2() + op.detune_cents / 100.0,
            c.rate,
        );
        let phase = t.oscillators[u][to].phase();
        let mut highest = inc * c.rate;
        let mut requested = op.feedback * op.level * 4.0 + lfo_index;
        for (source, depth) in fm[to].iter().enumerate() {
            if *depth > 0.0 {
                highest = highest
                    .max(core::increment(pitch + 12.0 * ops[source].ratio.log2(), c.rate) * c.rate);
                requested += depth * ops[source].level * 4.0;
            }
        }
        // Conservative Carson sideband budget against the host Nyquist. Large
        // indices roll down as keys rise; low musical indices remain unchanged.
        let budget = ((c.rate * 0.24 - inc * c.rate) / highest.max(1.0) - 1.0).max(0.0);
        let scale = if requested > 0.0 {
            (budget / requested).min(1.0)
        } else {
            1.0
        };
        let modulation = (fm[to]
            .iter()
            .zip(previous)
            .map(|(depth, source)| depth * source)
            .sum::<f32>()
            + op.feedback * previous[to]
            + lfo_index * c.lfo)
            * scale;
        // Keep phase modulation finite while preserving all small-signal routes.
        let moved = (phase + modulation / std::f32::consts::TAU).rem_euclid(1.0);
        let raw = crate::blocks::oscillator::tonal_sample(op.waveform.waveform(), moved, inc, 0.5);
        t.oscillators[u][to].tick(Waveform::Sine, inc, 0.5);
        let mut sample = raw;
        for (from, &depth) in ring[to].iter().enumerate() {
            // Depth 1 is a true ring product, depth 0 leaves the carrier intact.
            sample *= 1.0 - depth + depth * previous[from];
        }
        let key = (op.key_scale * (c.key as f32 - 60.0) / 12.0)
            .exp2()
            .min(4.0);
        now[to] = core::guard(sample * op.level * t.levels[to] * key);
    }
    t.previous[u] = now;
    now
}
