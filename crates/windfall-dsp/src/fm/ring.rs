use crate::blocks::oscillator::{pulse, saw, sine, triangle};
use crate::blocks::smooth::LinearRamp;
use crate::blocks::svf::SvfCoeffs;
use crate::instrument::Instrument;
use crate::param::ParamSet;

use super::common::{Bank, Controls, output, rate};
use super::{HybridWaveform, RingHybridParams};

/// Three tonal oscillators with FM, two ring products, resonant lowpass
/// and a post-filter amp envelope. Filter state belongs to each voice.
pub struct RingHybrid {
    sample_rate: f32,
    controls: Controls<RingHybridParams>,
    bank: Bank<3>,
    wave_mix: [[LinearRamp; 4]; 3],
}

fn wave_index(wave: HybridWaveform) -> usize {
    match wave {
        HybridWaveform::Sine => 0,
        HybridWaveform::Triangle => 1,
        HybridWaveform::Saw => 2,
        HybridWaveform::Square => 3,
    }
}

fn waveform(phase: f32, increment: f32, weights: [f32; 4]) -> f32 {
    let values = [
        sine(phase),
        triangle(phase, increment),
        saw(phase, increment),
        pulse(phase, increment, 0.5),
    ];
    values
        .iter()
        .zip(weights)
        .map(|(value, weight)| value * weight)
        .sum()
}

impl Default for RingHybrid {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            controls: Controls::default(),
            bank: Bank::default(),
            wave_mix: [[
                LinearRamp::new(1.0),
                LinearRamp::new(0.0),
                LinearRamp::new(0.0),
                LinearRamp::new(0.0),
            ]; 3],
        }
    }
}

impl Instrument for RingHybrid {
    type Params = RingHybridParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.controls.prepare(self.sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.bank.reset();
        self.controls.reset();
        for (mix, op) in self
            .wave_mix
            .iter_mut()
            .zip(self.controls.current.oscillators)
        {
            for (i, ramp) in mix.iter_mut().enumerate() {
                ramp.snap(if i == wave_index(op.waveform) {
                    1.0
                } else {
                    0.0
                });
            }
        }
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        let samples = self.controls.ramp_samples();
        self.controls.set(&p);
        for (mix, op) in self.wave_mix.iter_mut().zip(p.oscillators) {
            for (i, ramp) in mix.iter_mut().enumerate() {
                ramp.set_target(
                    if i == wave_index(op.waveform) {
                        1.0
                    } else {
                        0.0
                    },
                    samples,
                );
            }
        }
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
            let wave_weights = self
                .wave_mix
                .each_mut()
                .map(|mix| mix.each_mut().map(LinearRamp::tick));
            let q = 0.707 + 11.293 * p.resonance;
            let coeffs = SvfCoeffs::new(p.cutoff_hz, q, self.sample_rate);
            let mut mix = (0.0, 0.0);
            for voice in &mut self.bank.voices {
                if !voice.active {
                    continue;
                }
                let mut values = [0.0; 3];
                for i in (0..3).rev() {
                    let op = &p.oscillators[i];
                    let modulation = match i {
                        0 => p.fm_21 * values[1] + p.fm_31 * values[2],
                        1 => p.fm_32 * values[2],
                        _ => 0.0,
                    };
                    let step = (voice.frequency * op.ratio / self.sample_rate).min(0.24);
                    let phase =
                        (voice.phases[i] + modulation / std::f32::consts::TAU).rem_euclid(1.0);
                    voice.phases[i] = (voice.phases[i] + step).fract();
                    values[i] = waveform(phase, step, wave_weights[i]) * op.level;
                }
                let ring_12 = values[0] * (1.0 - p.ring_12 + p.ring_12 * values[1]);
                let ring_13 = ring_12 * (1.0 - p.ring_13 + p.ring_13 * values[2]);
                let input = (ring_13 + values[1] + values[2]) / 3.0;
                let filtered = voice.filter.tick(&coeffs, input).low;
                voice.filter.flush();
                let amp = voice.envelopes[0].tick(&p.amp_envelope, self.sample_rate);
                let sample = filtered * amp * voice.gain();
                let pan = voice.pan.tick();
                mix.0 += sample * pan.0;
                mix.1 += sample * pan.1;
                if voice.envelopes[0].idle() {
                    voice.active = false;
                    voice.filter.reset();
                }
            }
            *l = output(mix.0 * p.gain);
            *r = output(mix.1 * p.gain);
        }
    }
}
