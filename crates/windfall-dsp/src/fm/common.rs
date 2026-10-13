use crate::blocks::math::{clean, key_to_hz, ms_to_samples};
use crate::blocks::svf::Svf;
use crate::param::ParamSet;

use super::{FmEnvelopeParams, MAX_POLYPHONY};

pub(super) fn rate(value: f32) -> f32 {
    clean(value, 1.0, 384_000.0, 48_000.0)
}

/// Linear interpolation of copied controls, with exact arrival in 5 ms.
pub(super) struct Controls<P: ParamSet> {
    pub current: P,
    target: P,
    remaining: u32,
    samples: u32,
    fresh: bool,
}

impl<P: ParamSet> Default for Controls<P> {
    fn default() -> Self {
        Self {
            current: P::default(),
            target: P::default(),
            remaining: 0,
            samples: 240,
            fresh: true,
        }
    }
}

impl<P: ParamSet> Controls<P> {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.samples = ms_to_samples(5.0, sample_rate);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.current = self.target;
        self.remaining = 0;
        self.fresh = true;
    }
    pub fn set(&mut self, params: &P) {
        let target = params.sanitized();
        if target == self.target {
            return;
        }
        self.target = target;
        if self.fresh {
            self.current = self.target;
        } else {
            self.remaining = self.samples;
        }
    }
    pub fn tick(&mut self) -> P {
        self.fresh = false;
        if self.remaining > 0 {
            self.current
                .approach(&self.target, 1.0 / self.remaining as f32);
            self.remaining -= 1;
            if self.remaining == 0 {
                self.current = self.target;
            }
        }
        self.current
    }
    pub fn ramp_samples(&self) -> u32 {
        if self.fresh { 0 } else { self.samples }
    }
}

/// Linear ADSR with normalized progress. Changing a time does not restart
/// the segment; release ends at exact zero rather than an infinite tail.
#[derive(Clone, Copy, Default)]
pub(super) struct Envelope {
    stage: u8,
    progress: f32,
    value: f32,
    release_start: f32,
}

impl Envelope {
    pub fn start(&mut self) {
        *self = Self {
            stage: 1,
            ..Self::default()
        };
    }
    pub fn release(&mut self) {
        if self.stage != 0 && self.stage != 4 {
            self.release_start = self.value;
            self.progress = 0.0;
            self.stage = 4;
        }
    }
    pub fn idle(&self) -> bool {
        self.stage == 0
    }
    pub fn tick(&mut self, p: &FmEnvelopeParams, sample_rate: f32) -> f32 {
        let duration = match self.stage {
            1 => p.attack_ms,
            2 => p.decay_ms,
            4 => p.release_ms,
            _ => 1.0,
        };
        if matches!(self.stage, 1 | 2 | 4) {
            self.progress =
                (self.progress + 1.0 / (duration * 0.001 * sample_rate).max(1.0)).min(1.0);
        }
        match self.stage {
            1 => {
                self.value = self.progress;
                if self.progress >= 1.0 {
                    self.stage = 2;
                    self.progress = 0.0;
                }
            }
            2 => {
                self.value = 1.0 + (p.sustain - 1.0) * self.progress;
                if self.progress >= 1.0 {
                    self.stage = 3;
                }
            }
            3 => self.value = p.sustain,
            4 => {
                self.value = self.release_start * (1.0 - self.progress);
                if self.progress >= 1.0 {
                    self.stage = 0;
                    self.value = 0.0;
                }
            }
            _ => self.value = 0.0,
        }
        self.value
    }
}

#[derive(Clone, Copy)]
pub(super) struct Voice<const N: usize> {
    pub active: bool,
    pub key: u8,
    pub frequency: f32,
    pub velocity: f32,
    pub pan: crate::note_pan::NotePan,
    law: windfall_core::PanLaw,
    latest: bool,
    age: u64,
    pub phases: [f32; N],
    pub previous: [f32; N],
    pub older: [f32; N],
    pub envelopes: [Envelope; N],
    pub filter: Svf,
    choke_left: u32,
    choke_total: u32,
}

impl<const N: usize> Default for Voice<N> {
    fn default() -> Self {
        Self {
            active: false,
            key: 0,
            frequency: 440.0,
            velocity: 0.0,
            pan: crate::note_pan::NotePan::default(),
            law: windfall_core::PanLaw::Balance,
            latest: false,
            age: 0,
            phases: [0.0; N],
            previous: [0.0; N],
            older: [0.0; N],
            envelopes: [Envelope::default(); N],
            filter: Svf::default(),
            choke_left: 0,
            choke_total: 0,
        }
    }
}

impl<const N: usize> Voice<N> {
    pub fn advance(&mut self, operator: usize, ratio: f32, sample_rate: f32) -> f32 {
        let phase = self.phases[operator];
        let step = (self.frequency * ratio / sample_rate).min(0.45);
        self.phases[operator] = (phase + step).fract();
        phase
    }
    pub fn gain(&mut self) -> f32 {
        if self.choke_total == 0 {
            return self.velocity;
        }
        let gain = self.velocity * self.choke_left as f32 / self.choke_total as f32;
        self.choke_left = self.choke_left.saturating_sub(1);
        if self.choke_left == 0 {
            self.active = false;
        }
        gain
    }
    pub fn finish_envelopes(&mut self) {
        if self.envelopes.iter().all(Envelope::idle) {
            self.active = false;
        }
    }
}

pub(super) struct Bank<const N: usize> {
    pub voices: [Voice<N>; MAX_POLYPHONY],
}

impl<const N: usize> Default for Bank<N> {
    fn default() -> Self {
        Self {
            voices: [Voice::default(); MAX_POLYPHONY],
        }
    }
}

impl<const N: usize> Bank<N> {
    pub fn set_note_pan_law(&mut self, law: windfall_core::PanLaw, rate: f32) {
        for voice in &mut self.voices {
            voice.law = law;
            voice.pan.retarget(law, rate);
        }
    }
    pub fn pan_latest(&mut self, pan: f32) {
        if let Some(voice) = self.voices.iter_mut().find(|voice| voice.latest) {
            voice.pan = crate::note_pan::NotePan::new(pan, voice.law);
        }
    }

    pub fn reset(&mut self) {
        let law = self.voices[0].law;
        self.voices.fill(Voice {
            law,
            pan: crate::note_pan::NotePan::new(0.0, law),
            ..Voice::default()
        });
    }
    pub fn note_on(&mut self, key: u8, velocity: f32) {
        for voice in &mut self.voices { voice.latest = false; }
        let key = key.min(127);
        let velocity = clean(velocity, 0.0, 1.0, 0.0);
        if velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        for voice in &mut self.voices {
            if voice.active {
                voice.age = voice.age.saturating_add(1);
            }
        }
        let index = self
            .voices
            .iter()
            .position(|v| !v.active)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| v.age)
                    .map_or(0, |(i, _)| i)
            });
        let voice = &mut self.voices[index];
        *voice = Voice {
            active: true,
            key,
            frequency: key_to_hz(f32::from(key)),
            velocity,
            pan: crate::note_pan::NotePan::new(0.0, voice.law),
            law: voice.law,
            latest: true,
            ..Voice::default()
        };
        for env in &mut voice.envelopes {
            env.start();
        }
    }
    pub fn note_off(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.key == key.min(127) {
                for env in &mut voice.envelopes {
                    env.release();
                }
            }
        }
    }
    pub fn all_notes_off(&mut self, sample_rate: f32) {
        for voice in &mut self.voices {
            if voice.active && voice.choke_total == 0 {
                voice.choke_total = ms_to_samples(5.0, sample_rate);
                voice.choke_left = voice.choke_total;
            }
        }
    }
    pub fn active(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

pub(super) fn output(value: f32) -> f32 {
    clean(value, -64.0, 64.0, 0.0)
}
