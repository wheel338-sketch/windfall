use super::params::*;
use crate::blocks::math::{clean, ms_to_samples, smoothing_coefficient};
use crate::blocks::noise::Rng;
use crate::blocks::oscillator::{Oscillator, Waveform};
use crate::blocks::smooth::LinearRamp;
use crate::blocks::svf::{Svf, SvfCoeffs};
use crate::param::ParamSet;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Recipe {
    Membrane,
    Kick,
    Snare,
    Hat,
    Tom,
    Selected,
}

#[derive(Clone, Copy)]
pub(super) struct Patch {
    pitch: f32,
    decay: f32,
    tone: f32,
    noise_mix: f32,
    snap: f32,
    drop: f32,
    pitch_decay: f32,
    level: f32,
    strike: f32,
    damping: f32,
    body: f32,
}

pub(super) trait Settings: ParamSet {
    fn patch(&self, key: u8, pad: usize, recipe: Recipe) -> Patch;
    fn recipe(&self) -> Recipe {
        Recipe::Membrane
    }
}

fn tuning(key: u8, root: u8) -> f32 {
    ((f32::from(key) - f32::from(root)) / 12.0).exp2()
}

impl Settings for MembraneParams {
    fn patch(&self, key: u8, _: usize, _: Recipe) -> Patch {
        Patch {
            pitch: self.pitch_hz * tuning(key, 60),
            decay: self.decay_ms,
            tone: self.tone,
            noise_mix: 0.0,
            snap: self.snap,
            drop: self.pitch_drop_semitones,
            pitch_decay: self.pitch_decay_ms,
            level: self.level,
            strike: self.strike_position,
            damping: self.damping,
            body: self.body,
        }
    }
}
impl Settings for KickParams {
    fn patch(&self, key: u8, _: usize, _: Recipe) -> Patch {
        Patch {
            pitch: self.pitch_hz * tuning(key, 36),
            decay: self.decay_ms,
            tone: 0.5,
            noise_mix: 0.0,
            snap: self.click,
            drop: self.pitch_drop_semitones,
            pitch_decay: self.pitch_decay_ms,
            level: self.level,
            strike: 0.25,
            damping: 0.35,
            body: 0.4,
        }
    }
}
impl Settings for DrumRackParams {
    fn patch(&self, _: u8, pad: usize, _: Recipe) -> Patch {
        let pad = self.pads[pad];
        Patch {
            pitch: pad.pitch_hz,
            decay: pad.decay_ms,
            tone: pad.tone,
            noise_mix: pad.noise_mix,
            snap: 0.15,
            drop: 5.0,
            pitch_decay: 45.0,
            level: self.level,
            strike: 0.25,
            damping: 0.35,
            body: 0.4,
        }
    }
}
impl Settings for DrumVoiceParams {
    fn recipe(&self) -> Recipe {
        match self.mode {
            DrumMode::Kick => Recipe::Kick,
            DrumMode::Snare => Recipe::Snare,
            DrumMode::Hat => Recipe::Hat,
            DrumMode::Tom => Recipe::Tom,
        }
    }
    fn patch(&self, key: u8, _: usize, recipe: Recipe) -> Patch {
        let (pitch_scale, duration, drop, pitch_decay) = match recipe {
            Recipe::Kick => (0.5, 1.0, 30.0 + self.tone * 18.0, 16.0),
            Recipe::Snare => (1.5, 0.6, 3.0, 10.0),
            Recipe::Hat => (25.0, 0.25, 0.0, 1.0),
            _ => (1.0, 1.0, 9.0, 55.0),
        };
        Patch {
            pitch: self.pitch_hz * tuning(key, 36) * pitch_scale,
            decay: self.decay_ms * duration,
            tone: self.tone,
            noise_mix: 0.0,
            snap: self.snap,
            drop,
            pitch_decay,
            level: self.level,
            strike: 0.25,
            damping: 0.35,
            body: 0.4,
        }
    }
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    key: u8,
    pad: usize,
    recipe: Recipe,
    age: u64,
    velocity: f32,
    amp: f32,
    pitch_env: f32,
    transient: f32,
    modal_env: f32,
    osc: [Oscillator; 2],
    noise: Rng,
    filter: Svf,
    attack: LinearRamp,
    release: LinearRamp,
    correction: LinearRamp,
    last: f32,
    membrane: super::modal::ModalDrum,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            key: 0,
            pad: 0,
            recipe: Recipe::Membrane,
            age: 0,
            velocity: 0.0,
            amp: 0.0,
            pitch_env: 0.0,
            transient: 0.0,
            modal_env: 0.0,
            osc: [Oscillator::default(); 2],
            noise: Rng::new(0),
            filter: Svf::default(),
            attack: LinearRamp::new(0.0),
            release: LinearRamp::new(1.0),
            correction: LinearRamp::new(0.0),
            last: 0.0,
            membrane: super::modal::ModalDrum::default(),
        }
    }
}

impl Voice {
    fn start(&mut self, key: u8, pad: usize, recipe: Recipe, velocity: f32, seed: u32, rate: f32) {
        // Preserve the previous endpoint through a 4 ms correction when stolen
        // or retriggered, without adding a second voice or unbounded tail.
        let endpoint = if self.active { self.last } else { 0.0 };
        *self = Self {
            active: true,
            key,
            pad,
            recipe,
            velocity,
            amp: 1.0,
            pitch_env: 1.0,
            transient: 1.0,
            modal_env: 1.0,
            noise: Rng::new(seed),
            ..Self::default()
        };
        self.attack.set_target(1.0, ms_to_samples(0.5, rate));
        self.correction.snap(endpoint);
        self.correction.set_target(0.0, ms_to_samples(4.0, rate));
    }

    fn release(&mut self, time_ms: f32, rate: f32) {
        // Repeated note-offs must not prolong an already-started release.
        if self.active && (self.release.target() != 0.0 || time_ms <= 4.0) {
            if time_ms <= 4.0 {
                self.release.snap(self.release.value());
            }
            self.release.set_target(0.0, ms_to_samples(time_ms, rate));
            self.correction
                .set_target(0.0, ms_to_samples(time_ms.min(4.0), rate));
        }
    }

    fn tick(&mut self, patch: Patch, rate: f32) -> f32 {
        if !self.active {
            return 0.0;
        }
        let pitch = patch.pitch * (self.pitch_env * patch.drop / 12.0).exp2();
        let first = self.osc[0].tick(Waveform::Sine, pitch / rate, 0.5);
        let ratio = match self.recipe {
            Recipe::Snare => 1.73,
            Recipe::Hat => 1.483,
            Recipe::Tom => 1.5,
            _ => 1.593,
        };
        let second = self.osc[1].tick(Waveform::Sine, pitch * ratio / rate, 0.5);
        let cutoff = match self.recipe {
            Recipe::Snare => 1800.0 + 5000.0 * patch.tone,
            Recipe::Hat => 3500.0 + 4500.0 * patch.tone,
            Recipe::Kick => 4500.0,
            _ => 400.0 + 10000.0 * patch.tone,
        };
        let coeffs = SvfCoeffs::new(cutoff, 0.707, rate);
        let filtered = self.filter.tick(&coeffs, self.noise.bipolar());
        let sample = match self.recipe {
            Recipe::Kick => 0.95 * first + 0.3 * patch.snap * filtered.high * self.transient,
            Recipe::Snare => {
                let shell = 0.28 * (first + 0.4 * second * self.modal_env);
                let wires = (0.5 + 0.5 * patch.snap)
                    * (0.7 * filtered.high + 0.3 * filtered.band * coeffs.k);
                shell + wires
            }
            Recipe::Hat => {
                // Inharmonic ring modulation and high-passed noise, with a
                // much shorter envelope than the other recipes.
                0.25 * patch.snap * first * second + 0.75 * filtered.high
            }
            Recipe::Tom => {
                (first + 0.35 * patch.tone * second * self.modal_env) / (1.0 + 0.35 * patch.tone)
                    + 0.07 * patch.snap * filtered.low * self.transient
            }
            _ => {
                let body = self.membrane.tick(
                    pitch,
                    patch.decay,
                    patch.tone,
                    patch.strike,
                    patch.damping,
                    patch.body,
                    rate,
                );
                (1.0 - patch.noise_mix) * body
                    + patch.noise_mix * filtered.low
                    + 0.35 * patch.snap * filtered.low * self.transient
            }
        };
        let release = self.release.tick();
        let output = sample * self.amp * self.attack.tick() * release * self.velocity * patch.level
            + self.correction.tick();
        // Negative overshoot gives a finite exponential decay that reaches
        // exact zero at the requested duration, rather than a denormal tail.
        self.amp = -0.001 + (self.amp + 0.001) * (-6.908_755 / (patch.decay * 0.001 * rate)).exp();
        self.pitch_env *= (-1.0 / (patch.pitch_decay * 0.001 * rate)).exp();
        let strike_ms = match self.recipe {
            Recipe::Kick => 2.0,
            Recipe::Snare => 35.0,
            Recipe::Hat => 30.0,
            Recipe::Tom => 8.0,
            _ => 12.0,
        };
        self.transient *= (-1.0 / (strike_ms * 0.001 * rate)).exp();
        self.modal_env *= (-1.0 / (patch.decay * 0.000_25 * rate)).exp();
        // Flush auxiliary states long before subnormal arithmetic.
        self.pitch_env = crate::blocks::math::flush(self.pitch_env);
        self.transient = crate::blocks::math::flush(self.transient);
        self.modal_env = crate::blocks::math::flush(self.modal_env);
        self.filter.flush();
        self.age = self.age.saturating_add(1);
        self.last = output;
        if self.amp <= 0.0 && self.correction.is_settled() || release == 0.0 {
            *self = Self::default();
        } else {
            self.amp = self.amp.max(0.0);
        }
        output
    }
}

pub(super) struct Engine<P: Settings, const N: usize> {
    voices: [Voice; N],
    current: P,
    target: P,
    moving: bool,
    fresh: bool,
    rate: f32,
    smoothing: f32,
    seed: u32,
}

impl<P: Settings, const N: usize> Default for Engine<P, N> {
    fn default() -> Self {
        Self {
            voices: [Voice::default(); N],
            current: P::default(),
            target: P::default(),
            moving: false,
            fresh: true,
            rate: 48000.0,
            smoothing: smoothing_coefficient(10.0, 48000.0),
            seed: 0,
        }
    }
}

impl<P: Settings, const N: usize> Engine<P, N> {
    pub fn prepare(&mut self, rate: f32) {
        self.rate = clean(rate, 8000.0, 384000.0, 48000.0);
        self.smoothing = smoothing_coefficient(10.0, self.rate);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.voices = [Voice::default(); N];
        self.current = self.target;
        self.moving = false;
        self.fresh = true;
        self.seed = 0;
    }
    pub fn set_params(&mut self, params: &P) {
        self.target = params.sanitized();
        if self.fresh {
            self.current = self.target;
            self.moving = false;
        } else {
            self.moving = self.current != self.target;
        }
    }
    pub fn note_on(&mut self, key: u8, velocity: f32, recipe: Recipe, rack: bool) {
        if !velocity.is_finite() || velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        if key > 127 || (rack && !(36..52).contains(&key)) {
            return;
        }
        let pad = if rack { usize::from(key - 36) } else { 0 };
        let index = if rack {
            pad
        } else {
            self.voices
                .iter()
                .position(|v| !v.active)
                .unwrap_or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .max_by_key(|(_, v)| v.age)
                        .map_or(0, |(i, _)| i)
                })
        };
        let recipe = if recipe == Recipe::Selected {
            self.target.recipe()
        } else {
            recipe
        };
        self.seed = self.seed.wrapping_add(1);
        self.voices[index].start(key, pad, recipe, velocity.min(1.0), self.seed, self.rate);
    }
    pub fn note_off(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.key == key {
                voice.release(30.0, self.rate);
            }
        }
    }
    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices {
            voice.release(4.0, self.rate);
        }
    }
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            self.fresh = false;
            if self.moving {
                self.moving = self.current.approach(&self.target, self.smoothing);
            }
            let mut sum = 0.0;
            for voice in &mut self.voices {
                if voice.active {
                    let patch = self.current.patch(voice.key, voice.pad, voice.recipe);
                    sum += voice.tick(patch, self.rate);
                }
            }
            // Each bounded voice is finite; guard the boundary as well.
            let output = if sum.is_finite() { sum } else { 0.0 };
            *left = output;
            *right = output;
        }
    }
}
