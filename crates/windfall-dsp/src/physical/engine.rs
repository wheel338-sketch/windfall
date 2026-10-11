use std::f32::consts::TAU;

use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::{clean, flush, key_to_hz, smoothing_coefficient};
use crate::blocks::noise::Rng;
use crate::blocks::svf::{Svf, SvfCoeffs};

use super::MAX_POLYPHONY;

const MAX_LOOP_GAIN: f32 = 0.9998;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Model {
    Pluck,
    FingerBass,
    AcousticString,
}

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub decay: f32,
    pub brightness: f32,
    pub pick: f32,
    pub damping: f32,
    pub body: f32,
    pub stiffness: f32,
    pub sympathetic: f32,
    pub level: f32,
}

struct StringLoop {
    line: DelayLine,
    delay: f32,
    gain: f32,
    damping: f32,
    previous: f32,
    dispersive: bool,
    allpass_a: f32,
    allpass_b: f32,
    allpass_state: f32,
}

impl Default for StringLoop {
    fn default() -> Self {
        Self {
            line: DelayLine::default(),
            delay: 2.0,
            gain: 0.0,
            damping: 0.0,
            previous: 0.0,
            dispersive: false,
            allpass_a: 0.0,
            allpass_b: 1.0,
            allpass_state: 0.0,
        }
    }
}

impl StringLoop {
    fn reset(&mut self) {
        self.line.clear();
        self.previous = 0.0;
        self.allpass_state = 0.0;
    }

    fn configure(&mut self, frequency: f32, sr: f32, settings: Settings, dispersive: bool) {
        let w = TAU * frequency / sr;
        self.damping = 0.48 - 0.46 * settings.brightness;
        // FIR convex average: magnitude <= 1. Compensate its phase delay
        // at the fundamental instead of allowing brightness to detune notes.
        let phase_delay =
            (self.damping * w.sin()).atan2(1.0 - self.damping + self.damping * w.cos()) / w;
        self.dispersive = dispersive;
        self.allpass_a = -0.7 * settings.stiffness;
        self.allpass_b = (1.0 - self.allpass_a * self.allpass_a).sqrt();
        let dispersion_delay = if dispersive {
            2.0 * (((1.0 - self.allpass_a) / (1.0 + self.allpass_a)) * (w * 0.5).tan()).atan() / w
        } else {
            0.0
        };
        self.delay = (sr / frequency - phase_delay - dispersion_delay)
            .clamp(2.0, (self.line.max_delay() - 1) as f32);
        let decay = settings.decay / (1.0 + 5.0 * settings.damping);
        self.gain = (-std::f32::consts::LN_10 * 3.0 / (frequency * decay))
            .exp()
            .min(MAX_LOOP_GAIN);
    }

    fn excite(&mut self, pick: f32, seed: u32) {
        self.reset();
        let count = self.delay.ceil() as usize + 2;
        let mut rng = Rng::new(seed);
        let mut mean = 0.0;
        // A plucked displacement profile with a small deterministic noise
        // component. Pick position controls the harmonic notches.
        for i in 0..count {
            let x = i as f32 / count as f32;
            let triangle = if x < pick {
                x / pick
            } else {
                (1.0 - x) / (1.0 - pick)
            };
            let value = 0.8 * (triangle - 0.5) + 0.12 * rng.bipolar();
            self.line.push(value);
            mean += value;
        }
        mean /= count as f32;
        for _ in 0..count {
            let value = self.line.tap(count) - mean;
            self.line.push(value);
        }
    }

    #[inline]
    fn tick(&mut self, excitation: f32) -> f32 {
        // Linear interpolation is a convex read with no overshoot. Its
        // treble loss is intentional additional physical damping.
        let sample = self.line.tap_linear(self.delay);
        let mut feedback = (1.0 - self.damping) * sample + self.damping * self.previous;
        self.previous = flush(sample);
        if self.dispersive {
            // Orthogonal scattering allpass: x² + state² = y² + next²,
            // even while stiffness changes. No coefficient-change energy gain.
            let output = self.allpass_a * feedback + self.allpass_b * self.allpass_state;
            self.allpass_state =
                flush(self.allpass_b * feedback - self.allpass_a * self.allpass_state);
            feedback = output;
        }
        // A bounded bridge also limits exceptional modulation transients.
        self.line
            .push(flush((feedback * self.gain + excitation).clamp(-2.0, 2.0)));
        sample
    }
}

struct Voice {
    strings: [StringLoop; 2],
    body: [Svf; 2],
    body_coeffs: [SvfCoeffs; 2],
    key: u8,
    active: bool,
    serial: u64,
    velocity: f32,
    age: usize,
    quiet: usize,
    release_remaining: usize,
    release_total: usize,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            strings: std::array::from_fn(|_| StringLoop::default()),
            body: [Svf::default(); 2],
            body_coeffs: [SvfCoeffs::new(100.0, 2.0, 48_000.0); 2],
            key: 0,
            active: false,
            serial: 0,
            velocity: 0.0,
            age: 0,
            quiet: 0,
            release_remaining: 0,
            release_total: 0,
        }
    }
}

impl Voice {
    fn reset(&mut self) {
        for string in &mut self.strings {
            string.reset();
        }
        for filter in &mut self.body {
            filter.reset();
        }
        self.active = false;
        self.age = 0;
        self.quiet = 0;
        self.release_remaining = 0;
        self.release_total = 0;
    }

    fn configure(&mut self, model: Model, settings: Settings, sr: f32) {
        // Eight samples per cycle leave room for damping/dispersion phase
        // compensation even at the lowest supported sample rate.
        let frequency = key_to_hz(self.key as f32).min(sr / 8.0);
        self.strings[0].configure(frequency, sr, settings, model == Model::AcousticString);
        if model == Model::AcousticString {
            self.strings[1].configure(frequency * 2.0_f32.powf(3.0 / 1200.0), sr, settings, true);
        }
        if model == Model::FingerBass {
            self.body_coeffs = [
                SvfCoeffs::new(95.0 + 45.0 * settings.brightness, 2.5, sr),
                SvfCoeffs::new(310.0 + 190.0 * settings.brightness, 3.0, sr),
            ];
        }
    }

    fn release(&mut self, samples: usize) {
        if self.release_total == 0 || self.release_remaining > samples {
            // Keep the current gain when shortening an existing release.
            let gain = if self.release_total == 0 {
                1.0
            } else {
                self.release_remaining as f32 / self.release_total as f32
            };
            self.release_total = samples;
            self.release_remaining = (gain * samples as f32).ceil() as usize;
        }
    }

    fn tick(&mut self, model: Model, settings: Settings, sr: f32) -> f32 {
        let primary = self.strings[0].tick(0.0);
        let mut sample = primary;
        if model == Model::AcousticString {
            // One-way bridge coupling cannot form a second positive-feedback
            // cycle. Scale drive by loop loss to bound the secondary's gain.
            let drive = 0.2 * (1.0 - self.strings[1].gain) * primary;
            let second = self.strings[1].tick(drive);
            sample = (primary + 0.65 * settings.sympathetic * second)
                / (1.0 + 0.65 * settings.sympathetic);
        } else if model == Model::FingerBass {
            let mut body = 0.0;
            for (filter, coeffs) in self.body.iter_mut().zip(&self.body_coeffs) {
                body += filter.tick(coeffs, primary).band * coeffs.k;
                filter.flush();
            }
            sample = primary + settings.body * body * 0.75;
        }
        self.age += 1;
        if sample.abs() < 1.0e-6 {
            self.quiet += 1;
        } else {
            self.quiet = 0;
        }
        let quiet_limit = (self.strings[0].delay as usize * 2).max(2048);
        let attack = (self.age as f32 / (sr * 0.002)).min(1.0);
        let release = if self.release_total > 0 {
            let gain = self.release_remaining as f32 / self.release_total as f32;
            self.release_remaining = self.release_remaining.saturating_sub(1);
            if self.release_remaining == 0 {
                self.active = false;
            }
            gain
        } else {
            1.0
        };
        if self.quiet > quiet_limit {
            self.active = false;
        }
        let output = sample * self.velocity * attack * release;
        if output.is_finite() {
            output
        } else {
            self.reset();
            0.0
        }
    }
}

pub(super) struct Engine {
    model: Model,
    voices: [Voice; MAX_POLYPHONY],
    sample_rate: f32,
    prepared: bool,
    serial: u64,
    pub fresh: bool,
    pub until_control: usize,
    pub smoothing: f32,
    last_output: f32,
    retrigger: f32,
    retrigger_remaining: usize,
}

impl Engine {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            voices: std::array::from_fn(|_| Voice::default()),
            sample_rate: 48_000.0,
            prepared: false,
            serial: 0,
            fresh: true,
            until_control: 0,
            smoothing: 1.0,
            last_output: 0.0,
            retrigger: 0.0,
            retrigger_remaining: 0,
        }
    }

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = clean(sample_rate, 8000.0, 192_000.0, 48_000.0);
        let capacity = (self.sample_rate / key_to_hz(0.0)).ceil() as usize + 8;
        for voice in &mut self.voices {
            for string in &mut voice.strings {
                string.line = DelayLine::new(capacity);
            }
        }
        self.smoothing = smoothing_coefficient(
            10.0,
            self.sample_rate / crate::blocks::CONTROL_PERIOD as f32,
        );
        self.prepared = true;
    }

    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
        self.serial = 0;
        self.fresh = true;
        self.until_control = 0;
        self.last_output = 0.0;
        self.retrigger = 0.0;
        self.retrigger_remaining = 0;
    }

    pub fn note_on(&mut self, key: u8, velocity: f32, settings: Settings) {
        if !self.prepared {
            return;
        }
        // Bass is one string: a new key replaces the held/releasing string.
        // A bounded endpoint correction avoids a discontinuity on retrigger.
        let index = if self.model == Model::FingerBass {
            self.retrigger = self.last_output;
            self.retrigger_remaining = (self.sample_rate * 0.004).ceil() as usize;
            0
        } else {
            self.voices
                .iter()
                .position(|voice| !voice.active)
                .unwrap_or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, voice)| voice.serial)
                        .map_or(0, |(i, _)| i)
                })
        };
        self.serial = self.serial.wrapping_add(1);
        let voice = &mut self.voices[index];
        voice.reset();
        voice.key = key;
        voice.serial = self.serial;
        voice.velocity = velocity;
        voice.active = true;
        voice.configure(self.model, settings, self.sample_rate);
        let seed = (self.serial as u32).wrapping_mul(0x9e37_79b9) ^ u32::from(key);
        voice.strings[0].excite(settings.pick, seed);
        if self.model == Model::AcousticString {
            voice.strings[1].excite(settings.pick, seed ^ 0x85eb_ca6b);
        }
    }

    pub fn note_off(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.key == key {
                voice.release((self.sample_rate * 0.08) as usize);
            }
        }
    }

    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            if voice.active {
                voice.release((self.sample_rate * 0.004) as usize);
            }
        }
    }

    pub fn control(&mut self, settings: Settings) {
        for voice in &mut self.voices {
            if voice.active {
                voice.configure(self.model, settings, self.sample_rate);
            }
        }
    }

    pub fn tick(&mut self, settings: Settings) -> f32 {
        let mut output = 0.0;
        for voice in &mut self.voices {
            if voice.active {
                output += voice.tick(self.model, settings, self.sample_rate);
            }
        }
        // Fixed headroom at the eight-voice cap; stereo is centered mono.
        let mut output = output * settings.level * 0.25;
        if self.retrigger_remaining > 0 {
            let total = (self.sample_rate * 0.004).ceil().max(1.0);
            output += self.retrigger * self.retrigger_remaining as f32 / total;
            self.retrigger_remaining -= 1;
        }
        self.last_output = output;
        output
    }

    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }
}
