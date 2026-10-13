//! A bounded, single-voice formant instrument driven by synthetic phoneme codes.
//! This supplies vowel colors and consonant gestures, with no speech assets.

mod params;
pub use params::{MAX_PHONEMES, Phoneme, PhonemeBuffer, SpeechVoiceParams};

use crate::blocks::math::{clean, flush, smoothing_coefficient};
use crate::instrument::Instrument;
use crate::param::ParamSet;

#[derive(Clone, Copy, Default)]
struct Coefficients {
    real: f32,
    imaginary: f32,
    drive: f32,
}

#[derive(Clone, Copy, Default)]
struct Resonator {
    coefficients: Coefficients,
    real: f32,
    imaginary: f32,
}

impl Resonator {
    fn tick(&mut self, source: f32, target: Coefficients, glide: f32) -> f32 {
        let c = &mut self.coefficients;
        c.real += (target.real - c.real) * glide;
        c.imaginary += (target.imaginary - c.imaginary) * glide;
        c.drive += (target.drive - c.drive) * glide;
        // A complex pole pair. Convex interpolation keeps its radius below one.
        let real = c.real * self.real - c.imaginary * self.imaginary + c.drive * source;
        self.imaginary = flush(c.imaginary * self.real + c.real * self.imaginary);
        self.real = flush(real);
        self.real
    }
}

/// One monophonic pulse/noise source and three parallel formant resonators.
/// A new note replaces the old phrase. Phrase completion starts release.
pub struct SpeechVoice {
    params: SpeechVoiceParams,
    current: SpeechVoiceParams,
    moving: bool,
    sample_rate: f32,
    note_pan: crate::note_pan::NotePan,
    pan_law: windfall_core::PanLaw,
    glide: f32,
    table: [[Coefficients; 3]; 8],
    resonators: [Resonator; 3],
    key: u8,
    velocity: f32,
    phase: f64,
    progress: f64,
    position: usize,
    envelope: f32,
    release_step: f32,
    release_remaining: usize,
    active: bool,
    releasing: bool,
    noise: u32,
}

impl Default for SpeechVoice {
    fn default() -> Self {
        let mut voice = Self {
            params: SpeechVoiceParams::default(),
            current: SpeechVoiceParams::default(),
            moving: false,
            sample_rate: 48_000.0,
            note_pan: crate::note_pan::NotePan::default(),
            pan_law: windfall_core::PanLaw::Balance,
            glide: 1.0,
            table: [[Coefficients::default(); 3]; 8],
            resonators: [Resonator::default(); 3],
            key: 60,
            velocity: 0.0,
            phase: 0.0,
            progress: 0.0,
            position: 0,
            envelope: 0.0,
            release_step: 0.0,
            release_remaining: 0,
            active: false,
            releasing: false,
            noise: 0x6139_4a17,
        };
        voice.prepare(48_000.0, 0);
        voice
    }
}

impl SpeechVoice {
    pub const MAX_VOICES: usize = 1;
    pub const LATENCY_SAMPLES: usize = 0;

    fn phoneme(&self) -> Phoneme {
        if self.position < self.current.phrase.length as usize {
            self.current.phrase.codes[self.position]
        } else {
            Phoneme::Silence
        }
    }

    fn begin_release(&mut self, milliseconds: f32) {
        if self.active && !self.releasing {
            self.releasing = true;
            self.release_remaining =
                (milliseconds as f64 * 0.001 * self.sample_rate as f64).ceil() as usize;
            self.release_remaining = self.release_remaining.max(1);
            self.release_step = self.envelope / self.release_remaining as f32;
        }
    }

    fn tick(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        if self.moving {
            self.moving = self.current.approach(&self.params, self.glide);
        }
        if !self.releasing && self.position >= self.current.phrase.length as usize {
            self.begin_release(self.current.release_ms);
        }
        let phoneme = self.phoneme();
        let frequency = 440.0 * ((self.key as f32 + self.current.pitch - 69.0) / 12.0).exp2();
        let increment = (frequency / self.sample_rate).min(0.2) as f64;
        self.phase += increment;
        self.phase -= self.phase.floor();
        let pulse = if self.phase < 0.18 { 0.82 } else { -0.18 };
        self.noise ^= self.noise << 13;
        self.noise ^= self.noise >> 17;
        self.noise ^= self.noise << 5;
        let noise = (self.noise >> 8) as f32 / 8_388_608.0 - 1.0;
        let source = match phoneme {
            Phoneme::Silence => 0.0,
            Phoneme::NoiseBurst => noise * (-12.0 * self.progress as f32).exp(),
            Phoneme::Nasal => pulse * 0.6,
            _ => pulse,
        };
        let mut output = 0.0;
        for (resonator, target) in self.resonators.iter_mut().zip(self.table[phoneme as usize]) {
            output += resonator.tick(source, target, self.glide);
        }
        output *= 8.0;
        if phoneme == Phoneme::NoiseBurst {
            output += source * 0.2;
        }
        if phoneme == Phoneme::Silence {
            output = 0.0;
        }
        let sample = (output * self.envelope * self.velocity * self.current.level).tanh();
        if self.releasing {
            self.envelope = (self.envelope - self.release_step).max(0.0);
            self.release_remaining -= 1;
            if self.release_remaining == 0 {
                self.active = false;
                self.envelope = 0.0;
                self.resonators = [Resonator::default(); 3];
            }
        } else {
            self.envelope = (self.envelope + 1.0 / (0.005 * self.sample_rate)).min(1.0);
            self.progress += self.current.rate as f64 / self.sample_rate as f64;
            if self.progress >= 1.0 {
                self.progress -= 1.0;
                if self.position + 1 >= self.current.phrase.length as usize {
                    self.begin_release(self.current.release_ms);
                } else {
                    self.position += 1;
                }
            }
        }
        if sample.is_finite() { sample } else { 0.0 }
    }
}

impl Instrument for SpeechVoice {
    type Params = SpeechVoiceParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = clean(sample_rate, 8_000.0, 384_000.0, 48_000.0);
        self.glide = smoothing_coefficient(5.0, self.sample_rate);
        // Frequencies in Hz, bandwidth in Hz, relative levels. Rows follow Phoneme.
        let frequencies: [[f32; 3]; 8] = [
            [800.0, 1200.0, 2500.0],
            [800.0, 1200.0, 2500.0],
            [300.0, 2300.0, 3000.0],
            [300.0, 870.0, 2250.0],
            [530.0, 1850.0, 2500.0],
            [500.0, 1000.0, 2400.0],
            [1200.0, 2800.0, 3500.0],
            [250.0, 1000.0, 2000.0],
        ];
        for (row, frequencies) in self.table.iter_mut().zip(frequencies) {
            for (index, (coefficients, frequency)) in row.iter_mut().zip(frequencies).enumerate() {
                let radius =
                    (-std::f32::consts::PI * [90.0, 120.0, 180.0][index] / self.sample_rate).exp();
                let angle = std::f32::consts::TAU * frequency.min(self.sample_rate * 0.45)
                    / self.sample_rate;
                *coefficients = Coefficients {
                    real: radius * angle.cos(),
                    imaginary: radius * angle.sin(),
                    drive: (1.0 - radius) * [1.0, 0.65, 0.4][index],
                };
            }
        }
        self.reset();
    }

    fn reset(&mut self) {
        self.note_pan = crate::note_pan::NotePan::new(0.0, self.pan_law);
        self.current = self.params;
        self.moving = false;
        self.resonators = [Resonator::default(); 3];
        self.active = false;
        self.releasing = false;
        self.envelope = 0.0;
        self.release_remaining = 0;
        self.release_step = 0.0;
        self.phase = 0.0;
        self.progress = 0.0;
        self.position = 0;
        self.velocity = 0.0;
        self.noise = 0x6139_4a17;
    }

    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
        if self.active {
            self.current.phrase = self.params.phrase;
            self.moving = true;
        } else {
            self.current = self.params;
            self.moving = false;
        }
    }

    fn note_on(&mut self, key: u8, velocity: f32) {
        if !velocity.is_finite() || velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        self.reset();
        self.key = key.min(127);
        self.velocity = velocity.min(1.0);
        self.active = self.current.phrase.length > 0;
        let targets = self.table[self.phoneme() as usize];
        for (resonator, coefficients) in self.resonators.iter_mut().zip(targets) {
            resonator.coefficients = coefficients;
        }
    }

    fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        self.pan_law = law;
        self.note_pan.retarget(law, self.sample_rate);
    }
    fn note_on_expression(
        &mut self,
        key: u8,
        velocity: f32,
        pan: f32,
        _expression: crate::NoteExpression,
    ) {
        self.note_on(key, velocity);
        if velocity.is_finite() && velocity > 0.0 {
            self.note_pan = crate::note_pan::NotePan::new(pan, self.pan_law);
        }
    }
    fn note_off(&mut self, key: u8) {
        if key.min(127) == self.key {
            self.begin_release(self.current.release_ms);
        }
    }

    fn all_notes_off(&mut self) {
        // Also shorten an already releasing voice when transport stops.
        if self.active {
            self.releasing = false;
            self.begin_release(5.0);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            let sample = self.tick();
            let pan = self.note_pan.tick();
            *left = sample * pan.0;
            *right = sample * pan.1;
        }
    }

    fn active_voices(&self) -> usize {
        usize::from(self.active)
    }
}

#[cfg(test)]
mod tests;
