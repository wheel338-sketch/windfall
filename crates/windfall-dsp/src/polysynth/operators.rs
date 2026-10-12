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
    tone.matrix_cache.fill(MatrixCache::default());
    for (op, env) in operators.iter().zip(&mut tone.operators) {
        runtime::envelope(env, op.envelope, rate);
    }
}
#[derive(Clone, Copy)]
pub(super) struct MatrixCache {
    key: [f32; 3],
    increments: [f32; 6],
    scales: [f32; 6],
    gains: [f32; 6],
    fm_masks: [u32; 6],
    ring_masks: [u32; 6],
    sine_only: bool,
    ring_free: bool,
}
impl Default for MatrixCache {
    fn default() -> Self {
        Self {
            key: [f32::NAN; 3],
            increments: [0.0; 6],
            scales: [0.0; 6],
            gains: [0.0; 6],
            fm_masks: [0; 6],
            ring_masks: [0; 6],
            sine_only: false,
            ring_free: false,
        }
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
    let key = [pitch, c.rate, f32::from(c.key)];
    if t.matrix_cache[u].key != key {
        let cache = &mut t.matrix_cache[u];
        cache.key = key;
        cache.sine_only = ops.iter().all(|op| op.waveform == OperatorWave::Sine);
        cache.ring_free = ring.iter().flatten().all(|depth| *depth == 0.0);
        cache.increments = std::array::from_fn(|i| {
            core::increment(
                pitch + 12.0 * ops[i].ratio.log2() + ops[i].detune_cents / 100.0,
                c.rate,
            )
        });
        let sources: [f32; 6] =
            std::array::from_fn(|i| core::increment(pitch + 12.0 * ops[i].ratio.log2(), c.rate));
        for (to, op) in ops.iter().enumerate() {
            let inc = cache.increments[to];
            let mut highest = inc * c.rate;
            let mut requested = op.feedback * op.level * 4.0 + lfo_index;
            for (source, depth) in fm[to].iter().enumerate() {
                if *depth > 0.0 {
                    highest = highest.max(sources[source] * c.rate);
                    requested += depth * ops[source].level * 4.0;
                }
            }
            // Conservative Carson sideband budget against the host Nyquist. Large
            // indices roll down as keys rise; low musical indices remain unchanged.
            let budget = ((c.rate * 0.24 - inc * c.rate) / highest.max(1.0) - 1.0).max(0.0);
            cache.scales[to] = if requested > 0.0 {
                (budget / requested).min(1.0)
            } else {
                1.0
            };
            cache.gains[to] = (op.key_scale * (c.key as f32 - 60.0) / 12.0)
                .exp2()
                .min(4.0);
            cache.fm_masks[to] = fm[to]
                .iter()
                .enumerate()
                .fold(0, |bits, (i, depth)| bits | (u32::from(*depth != 0.0) << i));
            cache.ring_masks[to] = ring[to]
                .iter()
                .enumerate()
                .fold(0, |bits, (i, depth)| bits | (u32::from(*depth != 0.0) << i));
        }
    }
    let cache = &t.matrix_cache[u];
    if cache.sine_only && cache.ring_free {
        // Phase modulation depends only on the previous substep. Stage those
        // independent phases, then evaluate the six carriers as a SIMD-friendly
        // polynomial block. Two padded lanes are scratch, never extra voices.
        let mut phases = [0.0; 8];
        for to in 0..6 {
            let modulation = (route_sum(&fm[to], previous, cache.fm_masks[to])
                + ops[to].feedback * previous[to]
                + lfo_index * c.lfo)
                * cache.scales[to];
            let phase = t.oscillators[u][to].phase() + modulation / std::f32::consts::TAU;
            phases[to] = phase - phase.floor();
            t.oscillators[u][to].advance(cache.increments[to]);
        }
        for phase in &mut phases {
            *phase = crate::blocks::oscillator::sine_fast(*phase);
        }
        for to in 0..6 {
            now[to] = core::guard(phases[to] * ops[to].level * t.levels[to] * cache.gains[to]);
        }
        t.previous[u] = now;
        return now;
    }
    for (to, op) in ops.iter().enumerate() {
        let inc = cache.increments[to];
        let phase = t.oscillators[u][to].phase();
        let routed = route_sum(&fm[to], previous, cache.fm_masks[to]);
        let modulation =
            (routed + op.feedback * previous[to] + lfo_index * c.lfo) * cache.scales[to];
        // Keep phase modulation finite while preserving all small-signal routes.
        let moved = phase + modulation / std::f32::consts::TAU;
        let moved = moved - moved.floor();
        let raw = if op.waveform == OperatorWave::Sine {
            crate::blocks::oscillator::sine_fast(moved)
        } else {
            crate::blocks::oscillator::tonal_sample(op.waveform.waveform(), moved, inc, 0.5)
        };
        t.oscillators[u][to].advance(inc);
        let mut sample = raw;
        let mut mask = cache.ring_masks[to];
        if mask == 0x3f {
            for (from, &depth) in ring[to].iter().enumerate() {
                // Depth 1 is a true ring product, depth 0 leaves the carrier intact.
                sample *= 1.0 - depth + depth * previous[from];
            }
        } else {
            while mask != 0 {
                let source = mask.trailing_zeros() as usize;
                let depth = ring[to][source];
                sample *= 1.0 - depth + depth * previous[source];
                mask &= mask - 1;
            }
        }
        now[to] = core::guard(sample * op.level * t.levels[to] * cache.gains[to]);
    }
    t.previous[u] = now;
    now
}

#[inline]
fn route_sum(depths: &[f32; 6], previous: [f32; 6], mut mask: u32) -> f32 {
    if mask == 0x3f {
        depths
            .iter()
            .zip(previous)
            .map(|(depth, source)| depth * source)
            .sum()
    } else {
        let mut sum = 0.0;
        while mask != 0 {
            let source = mask.trailing_zeros() as usize;
            sum += depths[source] * previous[source];
            mask &= mask - 1;
        }
        sum
    }
}
