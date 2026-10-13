use crate::blocks::oscillator::sine;
use crate::instrument::Instrument;

use super::MatrixFmParams;
use super::common::{Bank, Controls, output, rate};

/// Six independently enveloped operators and a full signed 6 x 6 matrix.
/// Each route reads the previous sample, including cycles and diagonals.
pub struct MatrixFm {
    sample_rate: f32,
    controls: Controls<MatrixFmParams>,
    bank: Bank<6>,
}

impl Default for MatrixFm {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            controls: Controls::default(),
            bank: Bank::default(),
        }
    }
}

impl Instrument for MatrixFm {
    type Params = MatrixFmParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.controls.prepare(self.sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.bank.reset();
        self.controls.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.controls.set(params);
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        self.bank.note_on(key, velocity);
    }
    fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        self.bank.set_note_pan_law(law, self.sample_rate);
    }
    fn note_on_expression(
        &mut self,
        key: u8,
        velocity: f32,
        pan: f32,
        _expression: crate::NoteExpression,
    ) {
        self.note_on(key, velocity);
        self.bank.pan_latest(pan);
    }
    fn note_off(&mut self, key: u8) {
        self.bank.note_off(key);
    }
    fn all_notes_off(&mut self) {
        self.bank.all_notes_off(self.sample_rate);
    }
    fn active_voices(&self) -> usize {
        self.bank.active()
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        for (l, r) in left.iter_mut().zip(right) {
            let p = self.controls.tick();
            let normalization = p.operators.iter().map(|op| op.output).sum::<f32>().max(1.0);
            let mut mix = (0.0, 0.0);
            for voice in &mut self.bank.voices {
                if !voice.active {
                    continue;
                }
                let mut values = [0.0; 6];
                let mut sample = 0.0;
                for (i, value) in values.iter_mut().enumerate() {
                    let op = &p.operators[i];
                    let env = voice.envelopes[i].tick(&op.envelope, self.sample_rate);
                    let modulation = p.matrix[i]
                        .iter()
                        .zip(voice.previous)
                        .map(|(amount, source)| amount * source)
                        .sum::<f32>()
                        + op.feedback * 0.5 * (voice.previous[i] + voice.older[i]);
                    let phase = voice.advance(i, op.ratio, self.sample_rate);
                    let key_gain = (op.key_scaling * (f32::from(voice.key) - 69.0) / 12.0)
                        .exp2()
                        .min(4.0);
                    *value = sine(phase + modulation / std::f32::consts::TAU)
                        * env
                        * op.level
                        * key_gain;
                    sample += *value * op.output;
                }
                voice.older = voice.previous;
                voice.previous = values;
                let sample = sample * voice.gain() / normalization;
                let pan = voice.pan.tick();
                mix.0 += sample * pan.0;
                mix.1 += sample * pan.1;
                voice.finish_envelopes();
            }
            *l = output(mix.0 * p.gain);
            *r = output(mix.1 * p.gain);
        }
    }
}
