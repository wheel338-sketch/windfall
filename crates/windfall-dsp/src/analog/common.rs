use crate::blocks::adsr::Adsr;
use crate::blocks::math::{clean, key_to_hz, smoothing_coefficient};
use crate::blocks::noise::{PinkNoise, Rng};
use crate::blocks::oscillator::{MAX_INCREMENT, Oscillator, Waveform, sine};
use crate::blocks::svf::{Svf, SvfCoeffs};
use crate::param::ParamSet;

use super::{AnalogEnvelopeParams, MAX_POLYPHONY};

pub(super) fn rate(value: f32) -> f32 {
    clean(value, 8_000.0, 192_000.0, 48_000.0)
}

pub(super) fn increment(key: f32, sample_rate: f32) -> f32 {
    (key_to_hz(key.clamp(-24.0, 151.0)) / sample_rate).clamp(1.0e-7, MAX_INCREMENT)
}

pub(super) fn output(value: f32) -> f32 {
    if value.is_finite() { value.tanh() } else { 0.0 }
}

pub(super) struct Controls<P: ParamSet> {
    pub current: P,
    target: P,
    coefficient: f32,
    started: bool,
    moving: bool,
}

impl<P: ParamSet> Default for Controls<P> {
    fn default() -> Self {
        Self {
            current: P::default(),
            target: P::default(),
            coefficient: smoothing_coefficient(5.0, 48_000.0),
            started: false,
            moving: false,
        }
    }
}

impl<P: ParamSet> Controls<P> {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.coefficient = smoothing_coefficient(5.0, sample_rate);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.current = self.target;
        self.started = false;
        self.moving = false;
    }
    pub fn set(&mut self, params: &P) {
        self.target = params.sanitized();
        if self.started {
            // Discrete controls jump immediately; floats retain their value.
            self.moving = self.current.approach(&self.target, 0.0);
        } else {
            self.current = self.target;
        }
    }
    pub fn tick(&mut self) -> P {
        self.started = true;
        if self.moving {
            self.moving = self.current.approach(&self.target, self.coefficient);
        }
        self.current
    }
}

#[derive(Clone, Copy)]
pub(super) struct Voice {
    pub key: u8,
    pub held: bool,
    pub age: u8,
    pub velocity: f32,
    pub pan: crate::note_pan::NotePan,
    pub oscillators: [Oscillator; 3],
    pub rng: [Rng; 3],
    pub pink: [PinkNoise; 3],
    pub envelope: Adsr,
    pub filter: Svf,
    last: f32,
    stolen: f32,
    steal_remaining: u32,
    stop_remaining: Option<u32>,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            key: 0,
            held: false,
            age: 0,
            velocity: 0.0,
            pan: crate::note_pan::NotePan::default(),
            oscillators: [Oscillator::default(); 3],
            rng: [Rng::new(1), Rng::new(2), Rng::new(3)],
            pink: [PinkNoise::default(); 3],
            envelope: Adsr::default(),
            filter: Svf::default(),
            last: 0.0,
            stolen: 0.0,
            steal_remaining: 0,
            stop_remaining: None,
        }
    }
}

impl Voice {
    pub fn active(&self) -> bool {
        !self.envelope.is_idle() || self.steal_remaining > 0
    }
    pub fn oscillator(&mut self, index: usize, waveform: Waveform, inc: f32) -> f32 {
        match waveform {
            Waveform::WhiteNoise => self.rng[index].bipolar(),
            Waveform::PinkNoise => self.pink[index].tick(&mut self.rng[index]),
            _ => self.oscillators[index].tick(waveform, inc, 0.5),
        }
    }
    pub fn envelope(&mut self, params: AnalogEnvelopeParams, sample_rate: f32) -> f32 {
        self.envelope.configure(
            params.attack_ms,
            params.decay_ms,
            params.sustain,
            params.release_ms,
            sample_rate,
        );
        self.envelope.tick()
    }
    pub fn finish(
        &mut self,
        raw: f32,
        level: f32,
        cutoff: f32,
        resonance: f32,
        sample_rate: f32,
    ) -> f32 {
        let coeffs = SvfCoeffs::new(cutoff, 0.707 + resonance * 9.0, sample_rate);
        let filtered = self.filter.tick(&coeffs, raw).low;
        self.amplify(filtered, level, sample_rate)
    }
    pub fn amplify(&mut self, raw: f32, level: f32, sample_rate: f32) -> f32 {
        let mut value = raw * level * self.velocity;
        let fade_samples = (sample_rate * 0.005).round().max(1.0) as u32;
        if self.steal_remaining > 0 {
            value += self.stolen * self.steal_remaining as f32 / fade_samples as f32;
            self.steal_remaining -= 1;
        }
        if let Some(remaining) = self.stop_remaining {
            value *= remaining as f32 / fade_samples as f32;
            if remaining <= 1 {
                *self = Self::default();
                return 0.0;
            }
            self.stop_remaining = Some(remaining - 1);
        }
        if !self.active() {
            self.filter.reset();
            value = 0.0;
        }
        self.last = value;
        value
    }
}

#[derive(Default)]
pub(super) struct Bank {
    pub voices: [Voice; MAX_POLYPHONY],
    law: windfall_core::PanLaw,
    latest: Option<usize>,
}

impl Bank {
    pub fn set_note_pan_law(&mut self, law: windfall_core::PanLaw, rate: f32) {
        self.law = law;
        for voice in &mut self.voices { voice.pan.retarget(law, rate); }
    }
    pub fn pan_latest(&mut self, pan: f32) {
        if let Some(index) = self.latest { self.voices[index].pan = crate::note_pan::NotePan::new(pan, self.law); }
    }

    pub fn reset(&mut self) {
        self.latest = None;
        self.voices = [Voice::default(); MAX_POLYPHONY];
    }
    pub fn note_on(&mut self, key: u8, velocity: f32, sample_rate: f32) {
        self.latest = None;
        let velocity = clean(velocity, 0.0, 1.0, 0.0);
        if velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        // First idle slot, then oldest releasing voice, then oldest held.
        // Ages are bounded ranks, so there is no counter rollover.
        let slot = self
            .voices
            .iter()
            .position(|v| !v.active())
            .unwrap_or_else(|| {
                let released = self.voices.iter().any(|v| !v.held);
                let mut slot = 0;
                let mut oldest = 0;
                for (i, voice) in self.voices.iter().enumerate() {
                    if (!released || !voice.held) && voice.age >= oldest {
                        oldest = voice.age;
                        slot = i;
                    }
                }
                slot
            });
        let old = self.voices[slot];
        // Compact ranks after expired voices, then insert the newest at zero.
        let ranks = self.voices.map(|v| (v.active(), v.age));
        for (i, voice) in self.voices.iter_mut().enumerate() {
            if i != slot && voice.active() {
                voice.age = 1 + ranks
                    .iter()
                    .enumerate()
                    .filter(|&(j, &(active, age))| {
                        j != slot && active && (age < ranks[i].1 || (age == ranks[i].1 && j < i))
                    })
                    .count() as u8;
            }
        }
        let mut voice = Voice {
            key,
            held: true,
            velocity,
            stolen: old.last,
            steal_remaining: if old.active() {
                (sample_rate * 0.005).round() as u32
            } else {
                0
            },
            ..Voice::default()
        };
        voice.envelope.gate_on();
        voice.pan = crate::note_pan::NotePan::new(0.0, self.law);
        self.latest = Some(slot);
        self.voices[slot] = voice;
    }
    pub fn note_off(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.held && voice.key == key {
                voice.held = false;
                voice.envelope.gate_off();
            }
        }
    }
    pub fn stop(&mut self, sample_rate: f32) {
        for voice in &mut self.voices {
            if voice.active() {
                voice.held = false;
                voice.stop_remaining = Some((sample_rate * 0.005).round().max(1.0) as u32);
            }
        }
    }
    pub fn active(&self) -> usize {
        self.voices.iter().filter(|v| v.active()).count()
    }
}

pub(super) const TABLE_LEN: usize = 256;
const CAPS: [usize; 4] = [1, 2, 4, 8];

/// Three original harmonic shapes with four frequency-dependent mip levels.
#[derive(Clone, Copy)]
pub(super) struct Tables {
    data: [[[f32; TABLE_LEN]; 4]; 3],
}

impl Default for Tables {
    fn default() -> Self {
        Self {
            data: [[[0.0; TABLE_LEN]; 4]; 3],
        }
    }
}

impl Tables {
    pub fn generate(&mut self) {
        for shape in 0..3 {
            for (mip, &cap) in CAPS.iter().enumerate() {
                for n in 0..TABLE_LEN {
                    let phase = n as f32 / TABLE_LEN as f32;
                    let mut value = 0.0;
                    let mut weight = 0.0;
                    for harmonic in 1..=cap {
                        let amplitude = match shape {
                            0 => {
                                if harmonic == 1 {
                                    1.0
                                } else {
                                    0.0
                                }
                            }
                            1 => {
                                if harmonic % 2 == 1 {
                                    1.0 / harmonic as f32
                                } else {
                                    0.0
                                }
                            }
                            _ => 1.0 / harmonic as f32,
                        };
                        value += sine(phase * harmonic as f32) * amplitude;
                        weight += amplitude;
                    }
                    self.data[shape][mip][n] = value / weight;
                }
            }
        }
    }
    pub fn sample(&self, phase: f32, inc: f32, scan: f32) -> f32 {
        let mip = CAPS
            .iter()
            .rposition(|&cap| inc * (cap as f32) < 0.45)
            .unwrap_or(0);
        let position = phase * TABLE_LEN as f32;
        let index = position as usize % TABLE_LEN;
        let next = (index + 1) % TABLE_LEN;
        let fraction = position.fract();
        let scan = scan.clamp(0.0, 1.0) * 2.0;
        let shape = (scan as usize).min(1);
        let read = |s: usize| {
            let table = &self.data[s][mip];
            table[index] + (table[next] - table[index]) * fraction
        };
        let a = read(shape);
        a + (read(shape + 1) - a) * (scan - shape as f32)
    }
}
