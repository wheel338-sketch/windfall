use super::params::clean;
use super::{CrossfadeAxis, LoopMode, MAX_POLYPHONY, MAX_ZONES, Zone, ZoneTable};
use crate::{NoteExpression, NoteInstanceId};

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    id: NoteInstanceId,
    key: u8,
    serial: u64,
    zone: Zone,
    position: f64,
    step: f64,
    pitch: f32,
    velocity_gain: f32,
    /// Linear gain from the expression map of the channel. 1 is unity.
    gain: f32,
    pan: f32,
    expression: NoteExpression,
    envelope: f32,
    release_step: f32,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            id: NoteInstanceId(0),
            key: 0,
            serial: 0,
            zone: Zone::default(),
            position: 0.0,
            step: 1.0,
            pitch: 60.0,
            velocity_gain: 0.0,
            gain: 1.0,
            pan: 0.0,
            expression: NoteExpression::default(),
            envelope: 0.0,
            release_step: 0.0,
        }
    }
}
impl Voice {
    fn update_step(&mut self, sample_rate: f32) {
        let semitones = self.pitch - self.zone.root_key as f32
            + (self.zone.tune_cents + self.expression.fine_pitch_cents) / 100.0;
        let ratio = (semitones.clamp(-96.0, 96.0) / 12.0).exp2();
        self.step = (ratio * self.zone.sample.sample_rate / sample_rate) as f64;
    }
    fn release(&mut self, milliseconds: f32, sample_rate: f32) {
        let frames = (milliseconds * 0.001 * sample_rate).max(1.0);
        // Repeated note-offs never lengthen a release or emergency stop.
        self.release_step = self.release_step.max(self.envelope / frames);
    }
    fn tick(&mut self, law: Option<&windfall_core::PanLawRamp>, clock: u64) -> (f32, f32) {
        let len = self.zone.sample.len as usize;
        let looping = match self.zone.loop_mode {
            LoopMode::Off => false,
            LoopMode::Continuous => true,
            LoopMode::UntilRelease => self.release_step == 0.0,
        };
        let start = self.zone.loop_start as f64;
        let end = self.zone.loop_end as f64;
        if looping && self.position >= end {
            self.position = start + (self.position - start).rem_euclid(end - start);
        }
        if self.position >= len as f64 || len == 0 {
            self.active = false;
            return (0.0, 0.0);
        }
        let index = self.position as usize;
        let next = if looping && index + 1 >= self.zone.loop_end as usize {
            self.zone.loop_start as usize
        } else {
            (index + 1).min(len - 1)
        };
        let fraction = (self.position - index as f64) as f32;
        let a = self.zone.sample.data[index];
        let b = self.zone.sample.data[next];
        let value = (a + (b - a) * fraction)
            * self.envelope
            * self.velocity_gain
            * self.zone.gain
            * self.gain;
        // Zone spread is an instrument color; project law applies to the
        // note's pan, preserving the legacy constant-power zone center level.
        let (left, right) = if let Some(law) = law {
            let zone_left = ((1.0 - self.zone.pan) * 0.5).sqrt();
            let zone_right = ((1.0 + self.zone.pan) * 0.5).sqrt();
            let (note_left, note_right) = law.gains_at(clock, self.pan);
            (zone_left * note_left, zone_right * note_right)
        } else {
            // Direct DSP callers that select no project law retain their
            // historical zone+note equal-power pan behavior.
            let pan = (self.zone.pan + self.pan).clamp(-1.0, 1.0);
            (((1.0 - pan) * 0.5).sqrt(), ((1.0 + pan) * 0.5).sqrt())
        };
        self.position += self.step;
        if self.release_step > 0.0 {
            self.envelope = (self.envelope - self.release_step).max(0.0);
        }
        if self.envelope == 0.0 || (!looping && self.position >= len as f64) {
            self.active = false;
        }
        (value * left, value * right)
    }
}

pub(super) struct Engine {
    voices: Vec<Voice>,
    sample_rate: f32,
    serial: u64,
    legacy: u64,
    level: f32,
    target_level: f32,
    release_ms: f32,
    target_release_ms: f32,
    smoothing: f32,
    fresh: bool,
    note_law: Option<windfall_core::PanLawRamp>,
    selected_law: Option<windfall_core::PanLaw>,
    pan_clock: u64,
}
impl Default for Engine {
    fn default() -> Self {
        Self {
            voices: Vec::new(),
            sample_rate: 48_000.0,
            serial: 0,
            legacy: u64::MAX,
            level: 0.5,
            target_level: 0.5,
            release_ms: 100.0,
            target_release_ms: 100.0,
            smoothing: 1.0,
            fresh: true,
            note_law: None,
            selected_law: None,
            pan_clock: 0,
        }
    }
}
impl Engine {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = clean(sample_rate, 1000.0, 384_000.0, 48_000.0);
        self.smoothing = 1.0 - (-1.0 / (0.01 * self.sample_rate)).exp();
        self.voices.resize(MAX_POLYPHONY, Voice::default());
    }
    pub fn reset(&mut self, level: f32, release_ms: f32) {
        for voice in &mut self.voices {
            voice.active = false;
        }
        self.serial = 0;
        self.legacy = u64::MAX;
        self.fresh = true;
        self.pan_clock = 0;
        self.note_law = self.selected_law.map(windfall_core::PanLawRamp::at_rest);
        self.set_controls(level, release_ms);
    }
    pub fn set_controls(&mut self, level: f32, release_ms: f32) {
        self.target_level = level;
        self.target_release_ms = release_ms;
        if self.fresh {
            self.level = level;
            self.release_ms = release_ms;
        }
    }
    pub fn legacy_id(&mut self) -> NoteInstanceId {
        let id = NoteInstanceId(self.legacy);
        self.legacy = self.legacy.wrapping_sub(1);
        id
    }
    #[allow(clippy::too_many_arguments)]
    pub fn note_on(
        &mut self,
        id: NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
        table: &ZoneTable,
        axis: CrossfadeAxis,
    ) {
        if key > 127 || self.voices.is_empty() {
            return;
        }
        // A repeated instance identity retriggers its entire layer group.
        for voice in &mut self.voices {
            if voice.id == id {
                voice.active = false;
            }
        }
        let velocity = velocity.min(1.0);
        let mut weights = [0.0; MAX_ZONES];
        let mut matched = [false; MAX_ZONES];
        let mut count = 0;
        let mut sum = 0.0;
        for (index, zone) in table.zones.iter().take(table.len as usize).enumerate() {
            if zone.matches(key, velocity) {
                matched[index] = true;
                weights[index] = zone.weight(key, velocity, axis);
                sum += weights[index];
                count += 1;
            }
        }
        if count == 0 {
            return;
        }
        for (index, zone) in table.zones.iter().take(table.len as usize).enumerate() {
            if !matched[index] {
                continue;
            }
            let weight = if sum > 0.0 {
                weights[index] / sum
            } else {
                1.0 / count as f32
            };
            if weight == 0.0 {
                continue;
            }
            let slot = self
                .voices
                .iter()
                .position(|voice| !voice.active)
                .unwrap_or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .min_by_key(|(_, voice)| voice.serial)
                        .map(|(index, _)| index)
                        .unwrap_or(0)
                });
            self.serial = self.serial.wrapping_add(1);
            let voice = &mut self.voices[slot];
            *voice = Voice {
                active: true,
                id,
                key,
                serial: self.serial,
                zone: *zone,
                position: 0.0,
                step: 1.0,
                pitch: key as f32,
                velocity_gain: velocity * weight,
                gain: 1.0,
                pan: clean(pan, -1.0, 1.0, 0.0),
                expression: expression.clamped(),
                envelope: 1.0,
                release_step: 0.0,
            };
            voice.update_step(self.sample_rate);
        }
    }
    pub fn release_key(&mut self, key: u8) {
        for voice in &mut self.voices {
            if voice.active && voice.key == key {
                voice.release(
                    self.release_ms * voice.expression.release_multiplier(),
                    self.sample_rate,
                );
            }
        }
    }
    pub fn release_instance(&mut self, id: NoteInstanceId) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.release(
                    self.release_ms * voice.expression.release_multiplier(),
                    self.sample_rate,
                );
            }
        }
    }
    pub fn stop(&mut self) {
        for voice in &mut self.voices {
            if voice.active {
                voice.release(5.0, self.sample_rate);
            }
        }
    }
    pub fn expression(&mut self, id: NoteInstanceId, pan: f32, expression: NoteExpression) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.pan = clean(pan, -1.0, 1.0, 0.0);
                voice.expression = expression.clamped();
                voice.update_step(self.sample_rate);
            }
        }
    }
    pub fn gain(&mut self, id: NoteInstanceId, gain: f32) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.gain = clean(gain, 0.0, 16.0, 1.0);
            }
        }
    }
    pub fn pitch(&mut self, id: NoteInstanceId, pitch: f32) {
        for voice in &mut self.voices {
            if voice.active && voice.id == id {
                voice.pitch = clean(pitch, -128.0, 255.0, voice.key as f32);
                voice.update_step(self.sample_rate);
            }
        }
    }
    pub fn tick(&mut self) -> (f32, f32) {
        self.fresh = false;
        self.level = glide(self.level, self.target_level, self.smoothing, 2.0);
        self.release_ms = glide(
            self.release_ms,
            self.target_release_ms,
            self.smoothing,
            9999.0,
        );
        let mut left = 0.0;
        let mut right = 0.0;
        for voice in &mut self.voices {
            if voice.active {
                let (l, r) = voice.tick(self.note_law.as_ref(), self.pan_clock);
                left += l;
                right += r;
            }
        }
        self.pan_clock = self.pan_clock.saturating_add(1);
        (left * self.level, right * self.level)
    }
    pub fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        self.selected_law = Some(law);
        if let Some(ramp) = self.note_law.as_mut()
            && !self.fresh
        {
            ramp.retarget(
                law,
                self.pan_clock,
                (0.005 * self.sample_rate).round().max(1.0) as u64,
            );
        } else {
            self.note_law = Some(windfall_core::PanLawRamp::at_rest(law));
        }
    }
    pub fn active_voices(&self) -> usize {
        self.voices.iter().filter(|voice| voice.active).count()
    }
}
fn glide(current: f32, target: f32, amount: f32, span: f32) -> f32 {
    if (target - current).abs() <= span * 1.0e-4 {
        target
    } else {
        current + (target - current) * amount
    }
}
