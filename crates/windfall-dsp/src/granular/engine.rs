use super::params::clean;
use super::{MAX_GRAINS_PER_VOICE, MAX_POLYPHONY};
use super::{PlaybackAsset, SliceRegion};
use crate::{NoteExpression, NoteInstanceId};
use windfall_core::AudioBuffer;

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
    pub level: f32,
    pub pitch: f32,
    pub grain_ms: f32,
    pub density: f32,
    pub position: f32,
    pub spray: f32,
    pub seed: u32,
    pub duration_ms: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            key: 60,
            level: 0.5,
            pitch: 0.0,
            grain_ms: 80.0,
            density: 24.0,
            position: 0.5,
            spray: 1.0,
            seed: 1,
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
struct Region {
    gain: f32,
    pitch: f32,
    reverse: bool,
    attack_ms: f32,
    release_ms: f32,
}
impl From<&SliceRegion> for Region {
    fn from(s: &SliceRegion) -> Self {
        Self {
            gain: s.gain,
            pitch: s.pitch,
            reverse: s.reverse,
            attack_ms: s.attack_ms,
            release_ms: s.release_ms,
        }
    }
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
    deck: usize,
    region: Option<Region>,
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
            deck: 0,
            region: None,
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
    pub grains_dropped: u64,
    pub voices_stolen: u64,
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
            grains_dropped: 0,
            voices_stolen: 0,
        }
    }
    pub fn adopt(&mut self, previous: &Self) {
        // Preserve the newly prepared output rate and smoothing coefficient.
        self.voices = previous.voices;
        self.control_left = previous.control_left;
        self.legacy = previous.legacy;
        self.fresh = previous.fresh;
        self.grains_spawned = previous.grains_spawned;
        self.grains_dropped = previous.grains_dropped;
        self.voices_stolen = previous.voices_stolen;
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
        self.grains_dropped = 0;
        self.voices_stolen = 0;
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
        settings: Settings,
        asset: Option<&PlaybackAsset>,
    ) {
        if id.0 == 0 || !velocity.is_finite() || velocity <= 0.0 {
            return;
        }
        let key = key.min(127);
        let mut deck = 0;
        let region =
            if let Some(asset) = asset.filter(|_| matches!(self.mode, Mode::Map | Mode::Deck)) {
                let index = key as i16 - settings.key as i16;
                let index = if asset.legacy_key_wrap {
                    index.rem_euclid(asset.slices.len() as i16)
                } else {
                    index
                };
                if index < 0 || index as usize >= asset.slices.len() {
                    return;
                }
                let region = &asset.slices[index as usize];
                deck = region.deck as usize;
                Some(region)
            } else {
                None
            };
        let source = Source::new(asset, deck);
        let len = source.len();
        if len == 0 {
            return;
        }
        let start = (region.map_or(0.0, |s| s.start) as f64 * len as f64).floor() as usize;
        let end = if let Some(region) = region {
            (region.end as f64 * len as f64).floor() as usize
        } else {
            len
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
        if self.voices[index].active {
            self.voices_stolen = self.voices_stolen.saturating_add(1);
        }
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
            deck,
            region: region.map(Region::from),
            start,
            end,
            rng: settings.seed ^ (u32::from(key).wrapping_mul(0x9e3779b9)),
            ..Voice::default()
        };
    }
    pub fn release_key(&mut self, key: u8, ms: f32) {
        for voice in &mut self.voices {
            if voice.key == key {
                voice.release(samples(
                    voice.region.map_or(ms, |s| s.release_ms * ms / 30.0)
                        * voice.release_multiplier,
                    self.sample_rate,
                ));
            }
        }
    }
    pub fn release_instance(&mut self, id: NoteInstanceId, ms: f32) {
        for voice in &mut self.voices {
            if voice.id == id {
                voice.release(samples(
                    voice.region.map_or(ms, |s| s.release_ms * ms / 30.0)
                        * voice.release_multiplier,
                    self.sample_rate,
                ));
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
    pub fn tick(&mut self, settings: Settings, asset: Option<&PlaybackAsset>) -> (f32, f32) {
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
            let source = Source::new(asset, voice.deck);
            let len = source.len();
            if len == 0 {
                voice.active = false;
                continue;
            }
            let semitones = match self.mode {
                Mode::Map => voice.pitch - voice.key as f32 + voice.fine,
                Mode::Deck => {
                    voice.pitch - voice.key as f32
                        + voice.fine
                        + voice.region.map_or(0.0, |s| s.pitch)
                }
                _ => voice.pitch - settings.key as f32 + voice.fine + settings.pitch,
            };
            let ratio = 2.0f64.powf(semitones.clamp(-96.0, 96.0) as f64 / 12.0);
            let increment = ratio * source.rate() as f64 / self.sample_rate as f64;
            let signal = match self.mode {
                Mode::Map | Mode::Deck => {
                    let end = voice.end.min(len);
                    let span = end.saturating_sub(voice.start);
                    if voice.cursor >= span as f64 || span == 0 {
                        voice.active = false;
                        continue;
                    }
                    let reverse = voice.region.map_or(false, |s| s.reverse);
                    let position = if reverse {
                        end as f64 - 1.0 - voice.cursor
                    } else {
                        voice.start as f64 + voice.cursor
                    };
                    let signal = read_region(source, position, voice.start, end);
                    let edge = ((span as f64 - voice.cursor) / (increment * attack as f64)).min(1.0)
                        as f32;
                    voice.cursor += increment;
                    if voice.cursor >= span as f64 {
                        voice.active = false;
                    }
                    let gain = voice.region.map_or(1.0, |s| s.gain);
                    signal.map(|v| v * gain * edge)
                }
                Mode::Cloud => {
                    if voice.release_total == 0 {
                        if voice.phase >= 1.0 {
                            voice.phase -= 1.0;
                            let random = voice.random();
                            let position = (settings.position as f64
                                + (random - 0.5) * settings.spray as f64)
                                .rem_euclid(1.0)
                                * len as f64;
                            if let Some(grain) = voice.grains.iter_mut().find(|g| !g.active) {
                                *grain = Grain {
                                    active: true,
                                    position,
                                    age: 0,
                                    length: samples(settings.grain_ms, self.sample_rate).max(3),
                                };
                                self.grains_spawned = self.grains_spawned.saturating_add(1);
                            } else {
                                self.grains_dropped = self.grains_dropped.saturating_add(1);
                            }
                        }
                        voice.phase += settings.density as f64 / self.sample_rate as f64;
                    }
                    let mut sum = [0.0; 2];
                    for grain in &mut voice.grains {
                        if !grain.active {
                            continue;
                        }
                        let phase = grain.age as f64 / (grain.length - 1) as f64;
                        let window = (0.5 - 0.5 * (std::f64::consts::TAU * phase).cos()) as f32;
                        let sample = read_wrapped(source, grain.position);
                        for channel in 0..2 {
                            sum[channel] += sample[channel] * window;
                        }
                        grain.position = (grain.position + increment).rem_euclid(len as f64);
                        grain.age += 1;
                        if grain.age >= grain.length {
                            grain.active = false;
                        }
                    }
                    let scale = (settings.density * settings.grain_ms * 0.001)
                        .max(1.0)
                        .sqrt();
                    sum.map(|v| v / scale)
                }
                Mode::Wave => {
                    if voice.scan >= 1.0 {
                        voice.active = false;
                        continue;
                    }
                    let position =
                        asset.map_or(voice.scan, |a| a.curve(voice.key).position(voice.scan));
                    let delta =
                        ratio / (settings.duration_ms as f64 * 0.001 * self.sample_rate as f64);
                    let edge = ((1.0 - voice.scan) / (delta * attack as f64)).min(1.0) as f32;
                    voice.scan += delta;
                    if voice.scan >= 1.0 {
                        voice.active = false;
                    }
                    read_region(source, position * (len as f64 - 1.0), 0, len).map(|v| v * edge)
                }
            };
            let voice_attack = voice.region.map_or(attack, |s| {
                (s.attack_ms * 0.001 * self.sample_rate).max(1.0)
            });
            let mut envelope = (voice.age.saturating_add(1) as f32 / voice_attack).min(1.0);
            if voice.release_total != 0 {
                envelope *= voice.release_left as f32 / voice.release_total as f32;
                voice.release_left -= 1;
                if voice.release_left == 0 {
                    voice.active = false;
                }
            }
            voice.age = voice.age.saturating_add(1);
            let scale = envelope * voice.velocity * settings.level * 0.25 * voice.gain;
            for channel in 0..2 {
                bus[channel] += signal[channel] * scale * voice.pan[channel];
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
#[derive(Clone, Copy)]
enum Source<'a> {
    Audio(&'a AudioBuffer),
    Empty,
}
impl<'a> Source<'a> {
    fn new(asset: Option<&'a PlaybackAsset>, deck: usize) -> Self {
        match asset {
            Some(a) => a.sources[deck].as_ref().map_or(Self::Empty, Self::Audio),
            None => Self::Empty,
        }
    }
    fn len(self) -> usize {
        match self {
            Self::Audio(a) => a.frames(),
            Self::Empty => 0,
        }
    }
    fn rate(self) -> f32 {
        match self {
            Self::Audio(a) => a.sample_rate() as f32,
            Self::Empty => 48000.0,
        }
    }
    fn frame(self, frame: usize) -> [f32; 2] {
        match self {
            Self::Audio(a) => {
                let channels = usize::from(a.channels());
                let at = frame * channels;
                [a.samples()[at], a.samples()[at + usize::from(channels > 1)]]
            }
            Self::Empty => [0.0; 2],
        }
    }
}
fn read_region(source: Source<'_>, position: f64, start: usize, end: usize) -> [f32; 2] {
    let position = position.clamp(start as f64, (end - 1) as f64);
    let a = position.floor() as usize;
    let b = (a + 1).min(end - 1);
    interpolate(source, a, b, (position - a as f64) as f32)
}
fn read_wrapped(source: Source<'_>, position: f64) -> [f32; 2] {
    let position = position.rem_euclid(source.len() as f64);
    let a = position.floor() as usize;
    interpolate(
        source,
        a,
        (a + 1) % source.len(),
        (position - a as f64) as f32,
    )
}
fn interpolate(source: Source<'_>, a: usize, b: usize, fraction: f32) -> [f32; 2] {
    let a = source.frame(a);
    let b = source.frame(b);
    std::array::from_fn(|channel| a[channel] * (1.0 - fraction) + b[channel] * fraction)
}
