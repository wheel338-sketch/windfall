//! Asset-backed instruments. Banks are installed and destroyed off audio;
//! voices retain indexes into a bank and never own/drop individual Arcs.
mod asset;
mod params;
pub mod soundfont;
pub mod sources;
pub use asset::*;
pub use params::*;

use crate::{Instrument, NoteExpression, NoteInstanceId, ParamSet};
use std::sync::Arc;

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    id: NoteInstanceId,
    key: u8,
    zone: usize,
    position: f64,
    pitch: f32,
    source_pitch: f32,
    source_rate_ratio: f64,
    step: f64,
    pan_gains: [f32; 2],
    velocity: f32,
    pan: f32,
    gain: f32,
    age: u64,
    elapsed: u64,
    envelope: f32,
    release: f32,
    releasing: bool,
    filtered: [f32; 2],
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            id: NoteInstanceId(0),
            key: 0,
            zone: 0,
            position: 0.0,
            pitch: 0.0,
            source_pitch: 0.0,
            source_rate_ratio: 1.0,
            step: 1.0,
            pan_gains: [1.0; 2],
            velocity: 0.0,
            pan: 0.0,
            gain: 1.0,
            age: 0,
            elapsed: 0,
            envelope: 0.0,
            release: 0.0,
            releasing: false,
            filtered: [0.0; 2],
        }
    }
}

impl Voice {
    fn configure_pitch(&mut self, map: SampleZone, source_rate: u32, rate: f32) {
        self.source_pitch = (self.pitch - f32::from(map.root_key)) * map.scale_tuning / 100.0
            + map.tune_cents / 100.0;
        self.source_rate_ratio = f64::from(source_rate) / f64::from(rate);
        // The global transpose spans +/-48 semitones. Any source pitch outside
        // +/-144 is always saturated at the playback +/-96 bound.
        self.step = self.source_rate_ratio
            * f64::from((self.source_pitch.clamp(-144.0, 144.0) / 12.0).exp2());
    }
    fn configure_pan(&mut self, map: SampleZone) {
        let (left, right) = windfall_core::pan_gains((map.pan + self.pan).clamp(-1.0, 1.0));
        self.pan_gains = [left, right];
    }
}

struct Engine {
    bank: Option<Arc<SampleBank>>,
    voices: Box<[Voice]>,
    rate: f32,
    level: f32,
    target_level: f32,
    tune: f32,
    target_tune: f32,
    release_ms: f32,
    brightness: f32,
    sequence: u64,
    legacy: u64,
    rr: [u64; 256],
    started: bool,
    missed: u64,
    stolen: u64,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            bank: None,
            voices: vec![Voice::default(); BANK_VOICES].into_boxed_slice(),
            rate: 48000.0,
            level: 0.5,
            target_level: 0.5,
            tune: 0.0,
            target_tune: 0.0,
            release_ms: 100.0,
            brightness: 1.0,
            sequence: 0,
            legacy: u64::MAX,
            rr: [0; 256],
            started: false,
            missed: 0,
            stolen: 0,
        }
    }
}
impl Engine {
    fn release(&mut self, predicate: impl Fn(&Voice) -> bool, quick: bool) {
        let Some(bank) = &self.bank else {
            return;
        };
        for voice in &mut self.voices {
            if !voice.active || !predicate(voice) {
                continue;
            }
            let ms = if quick {
                5.0
            } else {
                bank.zones()[voice.zone].mapping.release_ms * self.release_ms / 100.0
            };
            let step = voice.envelope / (ms * 0.001 * self.rate).max(1.0);
            voice.release = if voice.releasing {
                voice.release.max(step)
            } else {
                step
            };
            voice.releasing = true;
        }
    }
    fn note(
        &mut self,
        id: NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
    ) {
        if key > 127 {
            return;
        }
        if !velocity.is_finite() || velocity <= 0.0 {
            self.release(|v| v.id == id, false);
            return;
        }
        let Some(bank) = &self.bank else {
            self.missed += 1;
            return;
        };
        let velocity = velocity.min(1.0);
        let expression = expression.clamped();
        let midi_velocity = (velocity * 127.0).round().max(1.0) as u8;
        let mut counts = [0usize; 256];
        for zone in bank.zones() {
            if zone.mapping.weight(key, midi_velocity) > 0.0 && zone.mapping.round_robin != 0 {
                counts[zone.mapping.round_robin as usize] += 1;
            }
        }
        let mut positions = [0usize; 256];
        let mut chokes = [false; 256];
        let mut selected = [false; MAX_BANK_ZONES];
        for (index, zone) in bank.zones().iter().enumerate() {
            let map = zone.mapping;
            if map.weight(key, midi_velocity) == 0.0 {
                continue;
            }
            let group = map.round_robin as usize;
            if group != 0 {
                let take = positions[group] == self.rr[group] as usize % counts[group];
                positions[group] += 1;
                if !take {
                    continue;
                }
            }
            selected[index] = true;
            if map.choke_group != 0 {
                chokes[map.choke_group as usize] = true;
            }
        }
        // Choke previous strikes once, before adding any layers of this strike.
        for voice in &mut self.voices {
            if voice.active
                && (voice.id == id || chokes[bank.zones()[voice.zone].mapping.choke_group as usize])
            {
                voice.active = false;
            }
        }
        let mut matched = false;
        for (index, zone) in bank.zones().iter().enumerate() {
            if !selected[index] {
                continue;
            }
            matched = true;
            let map = zone.mapping;
            let slot = self
                .voices
                .iter()
                .position(|v| !v.active)
                .unwrap_or_else(|| {
                    self.stolen += 1;
                    self.voices
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, v)| v.age)
                        .map_or(0, |(i, _)| i)
                });
            self.sequence = self.sequence.wrapping_add(1);
            self.voices[slot] = Voice {
                active: true,
                id,
                key,
                zone: index,
                pitch: f32::from(key) + expression.fine_pitch_cents / 100.0,
                velocity: velocity * zone.mapping.weight(key, midi_velocity),
                pan: if pan.is_finite() {
                    pan.clamp(-1.0, 1.0)
                } else {
                    0.0
                },
                age: self.sequence,
                ..Voice::default()
            };
            self.voices[slot].configure_pitch(map, zone.audio.sample_rate(), self.rate);
            self.voices[slot].configure_pan(map);
        }
        for (group, count) in counts.iter().enumerate() {
            if *count > 0 {
                self.rr[group] = self.rr[group].wrapping_add(1);
            }
        }
        if !matched {
            self.missed += 1;
        }
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
        let Some(bank) = &self.bank else {
            return;
        };
        let glide = 1.0 - (-1.0 / (0.01 * self.rate)).exp();
        let cutoff = (40.0 + self.brightness.powi(2) * 19960.0).min(self.rate * 0.45);
        let coefficient = 1.0 - (-std::f32::consts::TAU * cutoff / self.rate).exp();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.started = true;
            self.level += (self.target_level - self.level) * glide;
            self.tune += (self.target_tune - self.tune) * glide;
            let transpose = if self.tune == 0.0 {
                1.0
            } else {
                f64::from((self.tune / 12.0).exp2())
            };
            for voice in &mut self.voices {
                if !voice.active {
                    continue;
                }
                let zone = &bank.zones()[voice.zone];
                let map = zone.mapping;
                let looped = map.loop_mode == BankLoop::Continuous
                    || (map.loop_mode == BankLoop::UntilRelease && !voice.releasing);
                if looped && voice.position >= f64::from(map.loop_end) {
                    voice.position = f64::from(map.loop_start)
                        + (voice.position - f64::from(map.loop_start))
                            % f64::from(map.loop_end - map.loop_start);
                }
                if voice.position >= zone.audio.frames() as f64 {
                    voice.active = false;
                    continue;
                }
                if voice.releasing {
                    voice.envelope = (voice.envelope - voice.release).max(0.0);
                    if voice.envelope == 0.0 {
                        voice.active = false;
                        continue;
                    }
                } else {
                    let time = voice.elapsed as f32 / self.rate * 1000.0;
                    voice.envelope = if time < map.attack_ms {
                        time / map.attack_ms
                    } else if time < map.attack_ms + map.hold_ms {
                        1.0
                    } else if time < map.attack_ms + map.hold_ms + map.decay_ms {
                        1.0 + (map.sustain - 1.0) * (time - map.attack_ms - map.hold_ms)
                            / map.decay_ms
                    } else {
                        map.sustain
                    };
                }
                voice.elapsed += 1;
                let frame = voice.position as usize;
                let next = if looped && frame + 1 >= map.loop_end as usize {
                    map.loop_start as usize
                } else {
                    (frame + 1).min(zone.audio.frames() - 1)
                };
                let (a, b) = zone.audio.stereo_frame(frame);
                let (c, d) = zone.audio.stereo_frame(next);
                let fraction = (voice.position - frame as f64) as f32;
                voice.filtered[0] += (a + (c - a) * fraction - voice.filtered[0]) * coefficient;
                voice.filtered[1] += (b + (d - b) * fraction - voice.filtered[1]) * coefficient;
                let gain = voice.velocity * voice.gain * map.gain * voice.envelope * self.level;
                *l += voice.filtered[0] * gain * voice.pan_gains[0];
                *r += voice.filtered[1] * gain * voice.pan_gains[1];
                let pitch = voice.source_pitch + self.tune;
                voice.position += if pitch >= 96.0 {
                    voice.source_rate_ratio * 256.0
                } else if pitch <= -96.0 {
                    voice.source_rate_ratio / 256.0
                } else {
                    voice.step * transpose
                };
            }
        }
    }
}

macro_rules! instrument {
    ($name:ident, $params:ident) => {
        #[derive(Default)]
        pub struct $name {
            engine: Engine,
            params: $params,
        }
        impl $name {
            /// Off audio only. The host retires the entire old processor off audio.
            pub fn install_bank(&mut self, bank: Arc<SampleBank>) {
                self.reset();
                self.engine.bank = Some(bank);
            }
            pub fn missed_notes(&self) -> u64 {
                self.engine.missed
            }
            pub fn stolen_layers(&self) -> u64 {
                self.engine.stolen
            }
        }
        impl Instrument for $name {
            type Params = $params;
            fn prepare(&mut self, rate: f32, _block: usize) {
                self.engine.rate = if rate.is_finite() {
                    rate.clamp(1000.0, 384000.0)
                } else {
                    48000.0
                };
                self.reset();
            }
            fn reset(&mut self) {
                self.engine.voices.fill(Voice::default());
                self.engine.rr.fill(0);
                self.engine.started = false;
                self.engine.sequence = 0;
                let params = self.params;
                self.set_params(&params);
            }
            fn set_params(&mut self, params: &$params) {
                self.params = params.sanitized();
                self.engine.target_level = self.params.level;
                self.engine.target_tune = self.params.tune;
                self.engine.release_ms = self.params.release_ms;
                self.engine.brightness = self.params.brightness;
                if !self.engine.started {
                    self.engine.level = self.params.level;
                    self.engine.tune = self.params.tune;
                }
            }
            fn note_on(&mut self, key: u8, velocity: f32) {
                self.note_on_expression(key, velocity, 0.0, NoteExpression::default());
            }
            fn note_on_expression(
                &mut self,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: NoteExpression,
            ) {
                if !velocity.is_finite() || velocity <= 0.0 {
                    self.note_off(key);
                    return;
                }
                self.engine.legacy = self.engine.legacy.wrapping_sub(1);
                self.engine.note(
                    NoteInstanceId(self.engine.legacy),
                    key,
                    velocity,
                    pan,
                    expression,
                );
            }
            fn supports_note_instances(&self) -> bool {
                true
            }
            fn note_on_instance(
                &mut self,
                id: NoteInstanceId,
                key: u8,
                velocity: f32,
                pan: f32,
                expression: NoteExpression,
            ) {
                self.engine.note(id, key, velocity, pan, expression);
            }
            fn note_off(&mut self, key: u8) {
                self.engine.release(|v| v.key == key, false);
            }
            fn note_off_instance(&mut self, id: NoteInstanceId, _key: u8) {
                self.engine.release(|v| v.id == id, false);
            }
            fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
                if pitch.is_finite() {
                    for v in &mut self.engine.voices {
                        if v.active && v.id == id {
                            v.pitch = pitch.clamp(-48.0, 175.0);
                            if let Some(bank) = &self.engine.bank {
                                let zone = &bank.zones()[v.zone];
                                v.configure_pitch(
                                    zone.mapping,
                                    zone.audio.sample_rate(),
                                    self.engine.rate,
                                );
                            }
                        }
                    }
                }
            }
            fn set_note_expression(
                &mut self,
                id: NoteInstanceId,
                pan: f32,
                _expression: NoteExpression,
            ) {
                if pan.is_finite() {
                    for v in &mut self.engine.voices {
                        if v.active && v.id == id {
                            v.pan = pan.clamp(-1.0, 1.0);
                            if let Some(bank) = &self.engine.bank {
                                v.configure_pan(bank.zones()[v.zone].mapping);
                            }
                        }
                    }
                }
            }
            fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
                if gain.is_finite() {
                    for v in &mut self.engine.voices {
                        if v.active && v.id == id {
                            v.gain = gain.clamp(0.0, 4.0);
                        }
                    }
                }
            }
            fn all_notes_off(&mut self) {
                self.engine.release(|_| true, true);
            }
            fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
                self.engine.process(left, right);
            }
            fn active_voices(&self) -> usize {
                self.engine.voices.iter().filter(|v| v.active).count()
            }
        }
    };
}
instrument!(MultiSampler, MultiSamplerParams);
instrument!(SamplePlayer, SamplePlayerParams);
instrument!(SoundFont, SoundFontParams);
instrument!(PadBank, PadBankParams);
instrument!(PianoKeys, PianoKeysParams);
instrument!(TextVoice, TextVoiceParams);

#[cfg(test)]
mod tests;
