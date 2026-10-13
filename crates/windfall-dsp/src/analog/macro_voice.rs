use crate::blocks::oscillator::{Oscillator, Waveform, sine};
use crate::blocks::svf::SvfCoeffs;
use crate::instrument::Instrument;

use super::common::{Bank, Controls, Tables, increment, output, rate};
use super::{AnalogEnvelopeParams, MacroVoiceParams};

/// Four continuous macros blend a filtered oscillator, two-operator FM and a wavetable.
pub struct MacroVoice {
    sample_rate: f32,
    controls: Controls<MacroVoiceParams>,
    pub(super) bank: Bank,
    tables: Tables,
    motion: Oscillator,
}

impl Default for MacroVoice {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            controls: Controls::default(),
            bank: Bank::default(),
            tables: Tables::default(),
            motion: Oscillator::default(),
        }
    }
}

impl Instrument for MacroVoice {
    type Params = MacroVoiceParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.tables.generate();
        self.controls.prepare(self.sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.controls.reset();
        self.bank.reset();
        self.motion = Oscillator::default();
    }
    fn set_params(&mut self, params: &Self::Params) {
        self.controls.set(params);
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        self.bank.note_on(key, velocity, self.sample_rate);
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
        self.bank.stop(self.sample_rate);
    }
    fn active_voices(&self) -> usize {
        self.bank.active()
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.iter_mut().zip(right) {
            let p = self.controls.tick();
            let motion = self.motion.tick(
                Waveform::Sine,
                (0.2 + p.motion * 5.0) / self.sample_rate,
                0.5,
            );
            let position = p.engine_mix * 2.0;
            let weights = [
                (1.0 - position).max(0.0),
                1.0 - (position - 1.0).abs(),
                (position - 1.0).max(0.0),
            ];
            let envelope_params = AnalogEnvelopeParams {
                attack_ms: 2.0 + 45.0 * p.motion,
                decay_ms: 100.0 + 300.0 * p.shape,
                sustain: 0.5 + 0.4 * p.shape,
                release_ms: 80.0 + 600.0 * p.motion,
            };
            let mut sum = (0.0, 0.0);
            for voice in &mut self.bank.voices {
                if !voice.active() {
                    continue;
                }
                let inc = increment(voice.key as f32, self.sample_rate);
                let envelope = voice.envelope(envelope_params, self.sample_rate);
                let osc_phase = voice.oscillators[0].phase();
                let pulse = crate::blocks::oscillator::pulse(osc_phase, inc, 0.2 + p.shape * 0.6);
                let saw = voice.oscillators[0].tick(Waveform::Saw, inc, 0.5);
                let cutoff = 80.0 * (p.tone * 7.5 + envelope + motion * p.motion * 0.5).exp2();
                let coeffs = SvfCoeffs::new(cutoff, 0.707 + p.shape * 2.0, self.sample_rate);
                let filtered = voice
                    .filter
                    .tick(&coeffs, saw + (pulse - saw) * p.shape)
                    .low;
                let carrier_phase = voice.oscillators[1].phase();
                let modulator = voice.oscillators[2].tick(
                    Waveform::Sine,
                    (inc * (1.0 + p.shape * 3.0)).min(0.249),
                    0.5,
                );
                // Phase modulation in cycles: the modulator's index is bounded.
                let fm = sine(
                    carrier_phase
                        + modulator
                            * p.tone
                            * (0.1 + p.motion * 0.5)
                            * (1.0 + motion * p.motion * 0.2),
                );
                let scan =
                    (p.shape * (0.25 + p.tone * 0.75) + motion * p.motion * 0.15).clamp(0.0, 1.0);
                let table = self.tables.sample(carrier_phase, inc, scan);
                voice.oscillators[1].tick(Waveform::Sine, inc, 0.5);
                let raw = filtered * weights[0] + fm * weights[1] + table * weights[2];
                let sample = voice.amplify(raw, envelope, self.sample_rate);
                let pan = voice.pan.tick();
                sum.0 += sample * pan.0;
                sum.1 += sample * pan.1;
            }
            *left = output(sum.0 * 0.18);
            *right = if sum.0 == sum.1 {
                *left
            } else {
                output(sum.1 * 0.18)
            };
        }
    }
}
