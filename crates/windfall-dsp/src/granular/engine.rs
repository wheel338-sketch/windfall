use super::params::clean;
use super::{MAX_GRAINS_PER_VOICE, MAX_POLYPHONY, SLICE_COUNT, SampleTable, SliceSettings};
use crate::{NoteExpression, NoteInstanceId};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mode {
    Map,
    Deck,
    Cloud,
    Wave,
}

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub key: u8,
    pub starts: [f32; SLICE_COUNT],
    pub slices: [SliceSettings; SLICE_COUNT],
    pub level: f32,
    pub pitch: f32,
    pub grain_ms: f32,
    pub density: f32,
    pub position: f32,
    pub spray: f32,
    pub seed: u32,
    pub positions: [f32; 8],
    pub duration_ms: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            key: 60,
            starts: [0.0; SLICE_COUNT],
            slices: [SliceSettings::default(); SLICE_COUNT],
            level: 0.5,
            pitch: 0.0,
            grain_ms: 80.0,
            density: 24.0,
            position: 0.5,
            spray: 1.0,
            seed: 1,
            positions: [0.0; 8],
            duration_ms: 1000.0,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Grain {
    active: bool,
    position: f64,
    age: u32,
    length: u32,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    id: NoteInstanceId,
    key: u8,
    pitch: f32,
    fine: f32,
    release_multiplier: f32,
    velocity: f32,
    /// Linear gain from the expression map of the channel. 1 is unity.
    gain: f32,
    pan: [f32; 2],
    age: u64,
    release_left: u32,
    release_total: u32,
    slice: usize,
    start: usize,
    end: usize,
    cursor: f64,
    phase: f64,
    scan: f64,
    rng: u32,
    grains: [Grain; MAX_GRAINS_PER_VOICE],
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            id: NoteInstanceId(0),
            key: 60,
            pitch: 60.0,
            fine: 0.0,
            release_multiplier: 1.0,
            velocity: 0.0,
            gain: 1.0,
            pan: [1.0; 2],
            age: 0,
            release_left: 0,
            release_total: 0,
            slice: 0,
            start: 0,
            end: 0,
            cursor: 0.0,
            phase: 1.0,
            scan: 0.0,
            rng: 1,
            grains: [Grain::default(); MAX_GRAINS_PER_VOICE],
        }
    }
}
impl Voice {
    fn release(&mut self, samples: u32) {
        if self.active && (self.release_total == 0 || samples < self.release_left) {
            if self.release_total != 0 {
                self.velocity *= self.release_left as f32 / self.release_total as f32;
            }
            self.release_left = samples;
            self.release_total = samples;
        }
    }
    fn random(&mut self) -> f64 {
        self.rng = self.rng.wrapping_mul(1664525).wrapping_add(1013904223);
        (self.rng >> 8) as f64 / 16777216.0
    }
}

pub(super) struct Engine {
    mode: Mode,
    voices: [Voice; MAX_POLYPHONY],
    sample_rate: f32,
    control_left: usize,
    legacy: u64,
    pub fresh: bool,
    pub smoothing: f32,
    pub grains_spawned: u64,
}
impl Engine {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            voices: [Voice::default(); MAX_POLYPHONY],
            sample_rate: 48000.0,
            control_left: 0,
            legacy: 0,
            fresh: true,
            smoothing: 0.0328,
            grains_spawned: 0,
        }
    }
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = clean(sample_rate, 1000.0, 384000.0, 48000.0);
        self.smoothing = 1.0 - (-16.0 / (0.01 * self.sample_rate)).exp();
    }
    pub fn reset(&mut self) {
        self.voices.fill(Voice::default());
        self.control_left = 0;
        self.legacy = 0;
        self.grains_spawned = 0;
        self.fresh = true;
    }
    pub fn legacy_id(&mut self) -> NoteInstanceId {
        self.legacy = (self.legacy.wrapping_add(1) & (u64::MAX >> 1)).max(1);
        NoteInstanceId(self.legacy | (1 << 63))
    }
    pub fn control_due(&self) -> bool {
        self.control_left == 0
    }
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
    // Mirror instance-aware instrument note data, with the sample table borrowed
    // separately from compact settings so note-on never copies its fixed storage.
    #[allow(clippy::too_many_arguments)]
    pub fn note_on(
        &mut self,
        id: NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
        table: &SampleTable,
        settings: Settings,
    ) {
        if id.0 == 0 || !velocity.is_finite() || velocity <= 0.0 || table.len == 0 {
            return;
        }
        let key = key.min(127);
        let slice = (key as i16 - settings.key as i16).rem_euclid(SLICE_COUNT as i16) as usize;
        let len = usize::from(table.len);
        let start = (settings.starts[slice] as f64 * len as f64).floor() as usize;
        let end = if slice + 1 == SLICE_COUNT {
            len
        } else {
            (settings.starts[slice + 1] as f64 * len as f64).floor() as usize
        };
        if matches!(self.mode, Mode::Map | Mode::Deck) && end <= start {
            return;
        }
        let index = self
            .voices
            .iter()
            .position(|v| v.active && v.id == id)
            .or_else(|| self.voices.iter().position(|v| !v.active))
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, v)| v.age)
                    .unwrap()
                    .0
            });
        let expression = expression.clamped();
        self.voices[index] = Voice {
            active: true,
            id,
            key,
            pitch: key as f32,
            fine: expression.fine_pitch_cents / 100.0,
            release_multiplier: expression.release_multiplier(),
            velocity: velocity.min(1.0),
            pan: pan_gains(pan),
            slice,
            start,
            end,
            rng: settings.seed ^ (u32::from(key).wrapping_mul(0x9e3779b9)),
            ..Voice::default()
        };
    }
    pub fn release_key(&mut self, key: u8, ms: f32) {
        for voice in &mut self.voices {
            if voice.key == key {
                voice.release(samples(ms * voice.release_multiplier, self.sample_rate));
            }
        }
    }
    pub fn release_instance(&mut self, id: NoteInstanceId, ms: f32) {
        for voice in &mut self.voices {
            if voice.id == id {
                voice.release(samples(ms * voice.release_multiplier, self.sample_rate));
            }
        }
    }
    pub fn expression(&mut self, id: NoteInstanceId, pan: f32, expression: NoteExpression) {
        let expression = expression.clamped();
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.fine = expression.fine_pitch_cents / 100.0;
                voice.release_multiplier = expression.release_multiplier();
                voice.pan = pan_gains(pan);
            }
        }
    }
    pub fn gain(&mut self, id: NoteInstanceId, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.gain = gain.clamp(0.0, 16.0);
            }
        }
    }
    pub fn pitch(&mut self, id: NoteInstanceId, pitch: f32) {
        if !pitch.is_finite() {
            return;
        }
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.pitch = pitch.clamp(-12.0, 139.0) - voice.fine;
            }
        }
    }
    pub fn stop(&mut self) {
        for voice in &mut self.voices {
            voice.release(samples(4.0, self.sample_rate));
        }
    }
    pub fn tick(&mut self, table: &SampleTable, settings: Settings) -> (f32, f32) {
        self.fresh = false;
        if self.control_left == 0 {
            self.control_left = 16;
        }
        self.control_left -= 1;
        let mut bus = [0.0f32; 2];
        let attack = samples(1.0, self.sample_rate) as f32;
        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            if table.len == 0 {
                voice.active = false;
                continue;
            }
            let semitones = match self.mode {
                Mode::Map => voice.pitch - voice.key as f32 + voice.fine,
                Mode::Deck => {
                    voice.pitch - voice.key as f32 + voice.fine + settings.slices[voice.slice].pitch
                }
                _ => voice.pitch - settings.key as f32 + voice.fine + settings.pitch,
            };
            let ratio = 2.0f64.powf(semitones.clamp(-96.0, 96.0) as f64 / 12.0);
            let increment = ratio * table.sample_rate as f64 / self.sample_rate as f64;
            let signal = match self.mode {
                Mode::Map | Mode::Deck => {
                    let end = voice.end.min(table.len as usize);
                    let span = end.saturating_sub(voice.start);
                    if voice.cursor >= span as f64 || span == 0 {
                        voice.active = false;
                        continue;
                    }
                    let reverse = self.mode == Mode::Deck && settings.slices[voice.slice].reverse;
                    let position = if reverse {
                        end as f64 - 1.0 - voice.cursor
                    } else {
                        voice.start as f64 + voice.cursor
                    };
                    let signal = read_region(table, position, voice.start, end);
                    let edge = ((span as f64 - voice.cursor) / (increment * attack as f64)).min(1.0)
                        as f32;
                    voice.cursor += increment;
                    if voice.cursor >= span as f64 {
                        voice.active = false;
                    }
                    let gain = if self.mode == Mode::Deck {
                        settings.slices[voice.slice].gain
                    } else {
                        1.0
                    };
                    signal * gain * edge
                }
                Mode::Cloud => {
                    if voice.release_total == 0 {
                        if voice.phase >= 1.0 {
                            voice.phase -= 1.0;
                            let random = voice.random();
                            let position = (settings.position as f64
                                + (random - 0.5) * settings.spray as f64)
                                .rem_euclid(1.0)
                                * table.len as f64;
                            if let Some(grain) = voice.grains.iter_mut().find(|g| !g.active) {
                                *grain = Grain {
                                    active: true,
                                    position,
                                    age: 0,
                                    length: samples(settings.grain_ms, self.sample_rate).max(3),
                                };
                                self.grains_spawned = self.grains_spawned.saturating_add(1);
                            }
                        }
                        voice.phase += settings.density as f64 / self.sample_rate as f64;
                    }
                    let mut sum = 0.0;
                    for grain in &mut voice.grains {
                        if !grain.active {
                            continue;
                        }
                        let phase = grain.age as f64 / (grain.length - 1) as f64;
                        let window = (0.5 - 0.5 * (std::f64::consts::TAU * phase).cos()) as f32;
                        sum += read_wrapped(table, grain.position) * window;
                        grain.position = (grain.position + increment).rem_euclid(table.len as f64);
                        grain.age += 1;
                        if grain.age >= grain.length {
                            grain.active = false;
                        }
                    }
                    sum / (settings.density * settings.grain_ms * 0.001)
                        .max(1.0)
                        .sqrt()
                }
                Mode::Wave => {
                    if voice.scan >= 1.0 {
                        voice.active = false;
                        continue;
                    }
                    let point = voice.scan * 7.0;
                    let index = (point.floor() as usize).min(6);
                    let fraction = point - index as f64;
                    let position = settings.positions[index] as f64 * (1.0 - fraction)
                        + settings.positions[index + 1] as f64 * fraction;
                    let delta =
                        ratio / (settings.duration_ms as f64 * 0.001 * self.sample_rate as f64);
                    let edge = ((1.0 - voice.scan) / (delta * attack as f64)).min(1.0) as f32;
                    voice.scan += delta;
                    if voice.scan >= 1.0 {
                        voice.active = false;
                    }
                    read_region(
                        table,
                        position * (table.len as f64 - 1.0),
                        0,
                        table.len as usize,
                    ) * edge
                }
            };
            let mut envelope = (voice.age.saturating_add(1) as f32 / attack).min(1.0);
            if voice.release_total != 0 {
                envelope *= voice.release_left as f32 / voice.release_total as f32;
                voice.release_left -= 1;
                if voice.release_left == 0 {
                    voice.active = false;
                }
            }
            voice.age = voice.age.saturating_add(1);
            let signal = signal * envelope * voice.velocity * settings.level * 0.25 * voice.gain;
            for (out, gain) in bus.iter_mut().zip(voice.pan) {
                *out += signal * gain;
            }
        }
        (bus[0], bus[1])
    }
}
fn samples(ms: f32, rate: f32) -> u32 {
    (ms * 0.001 * rate).round().max(1.0) as u32
}
fn pan_gains(pan: f32) -> [f32; 2] {
    let angle = (clean(pan, -1.0, 1.0, 0.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    [angle.cos(), angle.sin()]
}
fn read_region(table: &SampleTable, position: f64, start: usize, end: usize) -> f32 {
    let position = position.clamp(start as f64, (end - 1) as f64);
    let a = position.floor() as usize;
    let b = (a + 1).min(end - 1);
    let fraction = (position - a as f64) as f32;
    table.data[a] * (1.0 - fraction) + table.data[b] * fraction
}
fn read_wrapped(table: &SampleTable, position: f64) -> f32 {
    // A table may be replaced with a shorter source while grains are sounding.
    let position = position.rem_euclid(table.len as f64);
    let a = position.floor() as usize;
    let b = (a + 1) % table.len as usize;
    let fraction = (position - a as f64) as f32;
    table.data[a] * (1.0 - fraction) + table.data[b] * fraction
}
