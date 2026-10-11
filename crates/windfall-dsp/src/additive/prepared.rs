//! Playback of immutable worker-prepared spectral frames. No analysis, table
//! synthesis, allocation, ownership replacement or destruction on audio.
use super::engine::sine_turns;
use crate::blocks::math::{clean, key_to_hz};
use crate::blocks::smooth::LinearRamp;
use crate::instrument_asset::SpectralBank;

#[derive(Clone, Copy)]
pub(super) struct Controls {
    pub position: f32,
    pub gain: f32,
    pub attack: f32,
    pub release: f32,
    pub polyphony: usize,
    pub cutoff: f32,
    pub resonance: f32,
    pub shape: u8,
    pub stretch: f32,
    pub shimmer: f32,
    pub shimmer_depth: f32,
    pub decay: f32,
    pub scan_rate: f32,
    pub scan: bool,
    pub pan_width: f32,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            position: 0.0,
            gain: 0.2,
            attack: 2.0,
            release: 200.0,
            polyphony: 8,
            cutoff: 20000.0,
            resonance: 0.0,
            shape: 0,
            stretch: 0.0,
            shimmer: 0.0,
            shimmer_depth: 0.0,
            decay: 0.0,
            scan_rate: 1.0,
            scan: false,
            pan_width: 1.0,
        }
    }
}
#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    key: u8,
    hz: f32,
    velocity: f32,
    age: u64,
    order: u64,
    phases: [f32; 64],
    release: LinearRamp,
    correction_left: LinearRamp,
    correction_right: LinearRamp,
    last_left: f32,
    last_right: f32,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            key: 0,
            hz: 0.0,
            velocity: 0.0,
            age: 0,
            order: 0,
            phases: [0.0; 64],
            release: LinearRamp::new(1.0),
            correction_left: LinearRamp::new(0.0),
            correction_right: LinearRamp::new(0.0),
            last_left: 0.0,
            last_right: 0.0,
        }
    }
}
pub(super) struct PreparedEngine {
    bank: Box<SpectralBank>,
    voices: [Voice; 16],
    controls: Controls,
    target: Controls,
    rate: f32,
    serial: u64,
    smoothing: f32,
    fresh: bool,
}
impl PreparedEngine {
    pub(super) fn new(bank: SpectralBank) -> Self {
        Self {
            bank: Box::new(bank),
            voices: [Voice::default(); 16],
            controls: Controls::default(),
            target: Controls::default(),
            rate: 48000.0,
            serial: 0,
            smoothing: 1.0 - (-1.0f32 / 240.0).exp(),
            fresh: true,
        }
    }
    pub(super) fn set_bank(&mut self, bank: &SpectralBank) {
        self.bank = Box::new(bank.clone());
    }
    pub(super) fn prepare(&mut self, rate: f32) {
        self.rate = clean(rate, 8000.0, 192000.0, 48000.0);
        self.smoothing = 1.0 - (-1.0 / (self.rate * 0.005)).exp();
        self.reset();
    }
    pub(super) fn reset(&mut self) {
        self.voices = [Voice::default(); 16];
        self.serial = 0;
        self.controls = self.target;
        self.fresh = true;
    }
    pub(super) fn set_controls(&mut self, controls: Controls) {
        self.target = controls;
        if self.fresh {
            self.controls = controls;
        }
    }
    pub(super) fn note_on(&mut self, key: u8, velocity: f32) {
        if key > 127 {
            return;
        }
        if !velocity.is_finite() || velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        let limit = self.target.polyphony.clamp(1, 16);
        let index = self.voices[..limit]
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                self.voices[..limit]
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, v)| v.order)
                    .map_or(0, |(i, _)| i)
            });
        self.serial = self.serial.wrapping_add(1);
        let mut voice = Voice {
            active: true,
            key,
            hz: key_to_hz(f32::from(key)),
            velocity: velocity.min(1.0),
            order: self.serial,
            ..Default::default()
        };
        for (phase, partial) in voice.phases.iter_mut().zip(&self.bank.frames[0].partials) {
            *phase = partial.phase;
        }
        if self.voices[index].active {
            voice.correction_left.snap(self.voices[index].last_left);
            voice.correction_right.snap(self.voices[index].last_right);
            voice
                .correction_left
                .set_target(0.0, (self.rate * 0.004).max(1.0) as u32);
            voice
                .correction_right
                .set_target(0.0, (self.rate * 0.004).max(1.0) as u32);
        }
        self.voices[index] = voice;
        for voice in &mut self.voices[limit..] {
            if voice.active {
                voice.release.set_target(0.0, (self.rate * 0.004) as u32);
            }
        }
    }
    pub(super) fn note_off(&mut self, key: u8) {
        for v in &mut self.voices {
            if v.active && v.key == key && v.release.target() != 0.0 {
                v.release.set_target(
                    0.0,
                    (self.rate * self.target.release * 0.001).max(1.0) as u32,
                );
            }
        }
    }
    pub(super) fn all_notes_off(&mut self) {
        for v in &mut self.voices {
            if v.active {
                v.release
                    .set_target(0.0, (self.rate * 0.004).max(1.0) as u32);
            }
        }
    }
    pub(super) fn voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
    pub(super) fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        for (left, right) in left.iter_mut().zip(right) {
            self.fresh = false;
            for (value, target) in [
                (&mut self.controls.position, self.target.position),
                (&mut self.controls.gain, self.target.gain),
                (&mut self.controls.cutoff, self.target.cutoff),
                (&mut self.controls.resonance, self.target.resonance),
                (&mut self.controls.stretch, self.target.stretch),
                (&mut self.controls.shimmer, self.target.shimmer),
                (&mut self.controls.shimmer_depth, self.target.shimmer_depth),
                (&mut self.controls.scan_rate, self.target.scan_rate),
                (&mut self.controls.pan_width, self.target.pan_width),
            ] {
                *value += (target - *value) * self.smoothing;
                if (*value - target).abs() < 1e-6 {
                    *value = target;
                }
            }
            let c = self.controls;
            let last = self.bank.frames.len() - 1;
            for v in &mut self.voices {
                if !v.active {
                    continue;
                }
                let seconds = v.age as f32 / self.rate;
                let position = if self.target.scan {
                    (seconds * c.scan_rate / self.bank.frame_seconds)
                        .rem_euclid(self.bank.frames.len() as f32)
                } else {
                    c.position * last as f32
                };
                let a = (position.floor() as usize).min(last);
                let b = (a + 1).min(last);
                let mix = position.fract();
                let attack = (seconds / (self.target.attack * 0.001).max(1.0 / self.rate)).min(1.0);
                let natural = if self.target.decay > 0.0 {
                    (-6.907755 * seconds / self.target.decay).exp()
                } else {
                    1.0
                };
                let envelope = attack * natural * v.release.tick() * v.velocity * c.gain;
                let (mut voice_left, mut voice_right) = (0.0, 0.0);
                for (i, (pa, pb)) in self.bank.frames[a]
                    .partials
                    .iter()
                    .zip(&self.bank.frames[b].partials)
                    .enumerate()
                {
                    let n = (i + 1) as f32;
                    let ratio = pa.ratio + (pb.ratio - pa.ratio) * mix;
                    let hz = v.hz * ratio * (1.0 + c.stretch * n * n * 0.001);
                    let increment = hz / self.rate;
                    let alias = ((0.49 - increment) / 0.05).clamp(0.0, 1.0);
                    let level = pa.level + (pb.level - pa.level) * mix;
                    let shape = match self.target.shape {
                        1 if i % 2 == 1 => 0.0,
                        2 if i % 2 == 1 => 0.0,
                        2 => {
                            if (i / 2) % 2 == 0 {
                                1.0 / n
                            } else {
                                -1.0 / n
                            }
                        }
                        _ => 1.0,
                    };
                    let filter = 1.0 / (1.0 + (hz / c.cutoff).powi(4));
                    let resonance = 1.0
                        + c.resonance * 4.0 * (1.0 - ((hz / c.cutoff - 1.0) * 3.0).abs()).max(0.0);
                    let shimmer = 1.0
                        + c.shimmer_depth
                            * sine_turns((seconds * c.shimmer + i as f32 * 0.137).fract());
                    let pan = ((pa.pan + (pb.pan - pa.pan) * mix) * c.pan_width).clamp(-1.0, 1.0);
                    let phase_offset = pa.phase - self.bank.frames[0].partials[i].phase
                        + (pb.phase - pa.phase) * mix;
                    let sample = sine_turns((v.phases[i] + phase_offset).rem_euclid(1.0))
                        * level
                        * shape
                        * filter
                        * resonance
                        * shimmer
                        * alias
                        * envelope;
                    voice_left += sample * (1.0 - pan) * 0.5;
                    voice_right += sample * (1.0 + pan) * 0.5;
                    v.phases[i] = (v.phases[i] + increment).rem_euclid(1.0);
                }
                v.last_left = voice_left + v.correction_left.tick();
                v.last_right = voice_right + v.correction_right.tick();
                *left += v.last_left;
                *right += v.last_right;
                v.age = v.age.saturating_add(1);
                if v.release.value() == 0.0 || natural < 1e-7 {
                    v.active = false;
                }
            }
            if !left.is_finite() {
                *left = 0.0;
            }
            if !right.is_finite() {
                *right = 0.0;
            }
        }
    }
}

macro_rules! prepared_instrument {
    ($name:ident, $params:ident, $bank:expr) => {
        pub struct $name {
            params: $params,
            engine: super::prepared::PreparedEngine,
        }
        impl Default for $name {
            fn default() -> Self {
                let params = $params::default();
                let mut engine = super::prepared::PreparedEngine::new($bank);
                engine.set_controls(params.controls());
                Self { params, engine }
            }
        }
        impl $name {
            pub(crate) fn set_bank(&mut self, bank: &crate::instrument_asset::SpectralBank) {
                self.engine.set_bank(bank);
            }
        }
        impl crate::Instrument for $name {
            type Params = $params;
            fn prepare(&mut self, rate: f32, _max_block: usize) {
                self.engine.prepare(rate);
            }
            fn reset(&mut self) {
                self.engine.reset();
            }
            fn set_params(&mut self, params: &Self::Params) {
                self.params = params.sanitized();
                self.engine.set_controls(self.params.controls());
            }
            fn note_on(&mut self, key: u8, velocity: f32) {
                self.engine.note_on(key, velocity);
            }
            fn note_off(&mut self, key: u8) {
                self.engine.note_off(key);
            }
            fn all_notes_off(&mut self) {
                self.engine.all_notes_off();
            }
            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                self.engine.process(left, right);
            }
            fn active_voices(&self) -> usize {
                self.engine.voices()
            }
        }
    };
}
pub(super) use prepared_instrument;
