//! What the instruments of this family share: smoothed controls, the voice
//! allocator with glide and de-clicked stealing, the ladder and saturating
//! state-variable filters, and the small helpers around oscillator phases.
//!
//! Everything here is plain inline state. Nothing allocates.

use std::f32::consts::TAU;

use crate::blocks::math::{clean, flush, key_to_hz, ms_to_samples, smoothing_coefficient};
use crate::blocks::oscillator::MAX_INCREMENT;
use crate::blocks::svf::cutoff_gain;
use crate::param::ParamSet;

/// The most voices any instrument of the family sounds at once, releases
/// and stop fades included.
pub const MAX_VOICES: usize = 16;

/// Length of the fade that stops a voice at once, and of the ramp that
/// hides a stolen voice's last sample.
pub(crate) const FADE_MS: f32 = 5.0;

/// A sample rate the family can run at. Anything else becomes 48 kHz.
pub(crate) fn rate(value: f32) -> f32 {
    clean(value, 8_000.0, 192_000.0, 48_000.0)
}

/// Phase step per sample of a fractional key.
#[inline]
pub(crate) fn increment(key: f32, sample_rate: f32) -> f32 {
    (key_to_hz(key.clamp(-36.0, 160.0)) / sample_rate).clamp(1.0e-7, MAX_INCREMENT)
}

/// Rolls phase modulation down before its estimated sidebands reach the host
/// Nyquist. Increments are measured at the family's doubled internal rate.
pub(crate) fn phase_index(carrier: f32, modulator: f32, index: f32) -> f32 {
    let budget = ((0.24 - carrier) / modulator.max(1.0e-7) - 1.0).max(0.0);
    index.clamp(-budget, budget)
}

/// Keeps a broken sample out of the mix bus.
#[inline]
pub(crate) fn guard(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-4.0, 4.0)
    } else {
        0.0
    }
}

/// A filter cutoff moved by `octaves`, held inside the range the filters
/// stay tuned in.
#[inline]
pub(crate) fn swept(cutoff_hz: f32, octaves: f32, sample_rate: f32) -> f32 {
    let top = (sample_rate * 0.45).min(20_000.0);
    (cutoff_hz * octaves.clamp(-12.0, 12.0).exp2()).clamp(16.0, top)
}

/// The filter integrator gain of [`swept`].
#[inline]
pub(crate) fn swept_gain(cutoff_hz: f32, octaves: f32, sample_rate: f32) -> f32 {
    cutoff_gain(swept(cutoff_hz, octaves, sample_rate), sample_rate)
}

/// The stored settings and the smoothed ones a processor reads.
///
/// Settings given before the first sample apply at once. Later ones glide:
/// continuous values close 63% of the gap every 5 ms, switches and whole
/// numbers jump.
pub(crate) struct Controls<P: ParamSet> {
    pub current: P,
    target: P,
    coefficient: f32,
    started: bool,
    moving: bool,
    changed: bool,
}

impl<P: ParamSet> Default for Controls<P> {
    fn default() -> Self {
        Self {
            current: P::default(),
            target: P::default(),
            coefficient: smoothing_coefficient(FADE_MS, 48_000.0),
            started: false,
            moving: false,
            changed: true,
        }
    }
}

impl<P: ParamSet> Controls<P> {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.coefficient = smoothing_coefficient(FADE_MS, sample_rate);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.current = self.target;
        self.started = false;
        self.moving = false;
        self.changed = true;
    }

    pub fn set(&mut self, params: &P) {
        let target = params.sanitized();
        if target == self.target {
            return;
        }
        self.target = target;
        self.changed = true;
        if self.started {
            // Switches and whole numbers jump now; continuous values stay.
            self.moving = self.current.approach(&self.target, 0.0);
        } else {
            self.current = self.target;
        }
    }

    /// The settings to end up at, already forced into range.
    pub fn target(&self) -> &P {
        &self.target
    }

    /// The settings for this sample, and whether they differ from the ones
    /// of the sample before.
    #[inline]
    pub fn tick(&mut self) -> (&P, bool) {
        self.started = true;
        let changed = self.changed || self.moving;
        self.changed = false;
        if self.moving {
            self.moving = self.current.approach(&self.target, self.coefficient);
        }
        (&self.current, changed)
    }
}

/// One slot of the voice allocator: which key it plays, where its pitch
/// is, and the fades that keep starting and stopping it free of clicks.
///
/// The instrument keeps the sound-making state of the slot beside it and
/// answers the `start` and `release` requests on the next sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Note {
    pub active: bool,
    pub held: bool,
    pub key: u8,
    pub velocity: f32,
    /// Larger is newer.
    pub order: u32,
    /// The pitch right now, in fractional keys.
    pub pitch: f32,
    target: f32,
    glide_left: u32,
    glide_step: f32,
    /// Asks the instrument to restart the slot's voice. The instrument
    /// clears it.
    pub start: bool,
    /// Asks the instrument to begin the voice's release. The instrument
    /// clears it.
    pub release: bool,
    fade_left: u32,
    fade_total: u32,
    carry: [f32; 2],
    carry_left: u32,
    carry_total: u32,
    last: [f32; 2],
}

impl Note {
    pub const IDLE: Self = Self {
        active: false,
        held: false,
        key: 0,
        velocity: 0.0,
        order: 0,
        pitch: 60.0,
        target: 60.0,
        glide_left: 0,
        glide_step: 0.0,
        start: false,
        release: false,
        fade_left: 0,
        fade_total: 0,
        carry: [0.0; 2],
        carry_left: 0,
        carry_total: 0,
        last: [0.0; 2],
    };

    /// Moves the glide on by one sample and returns the pitch.
    #[inline]
    pub fn glide(&mut self) -> f32 {
        if self.glide_left > 0 {
            self.glide_left -= 1;
            self.pitch = if self.glide_left == 0 {
                self.target
            } else {
                self.pitch + self.glide_step
            };
        }
        self.pitch
    }

    fn retarget(&mut self, pitch: f32, samples: u32) {
        self.target = pitch;
        if samples == 0 {
            self.pitch = pitch;
            self.glide_left = 0;
        } else {
            self.glide_left = samples;
            self.glide_step = (pitch - self.pitch) / samples as f32;
        }
    }

    /// True while the slot is being faded out to stop at once.
    pub fn stopping(&self) -> bool {
        self.fade_total > 0
    }

    /// Ends one sample of the slot: applies the stop fade, adds what is
    /// left of a stolen voice, and frees the slot once nothing sounds.
    /// `sounding` is false once the voice's own envelope has run out.
    #[inline]
    pub fn finish(&mut self, left: f32, right: f32, sounding: bool) -> (f32, f32) {
        let mut out = if sounding { [left, right] } else { [0.0, 0.0] };
        let mut alive = sounding;
        if self.fade_total > 0 {
            let gain = self.fade_left as f32 / self.fade_total as f32;
            out[0] *= gain;
            out[1] *= gain;
            self.fade_left = self.fade_left.saturating_sub(1);
            if self.fade_left == 0 {
                alive = false;
            }
        }
        if self.carry_left > 0 {
            let gain = self.carry_left as f32 / self.carry_total.max(1) as f32;
            out[0] += self.carry[0] * gain;
            out[1] += self.carry[1] * gain;
            self.carry_left -= 1;
        }
        self.last = out;
        if !alive && self.carry_left == 0 {
            *self = Self::IDLE;
        } else if !alive {
            // Only the stolen voice's ramp is left.
            self.held = false;
            self.fade_total = 1;
            self.fade_left = 0;
        }
        (out[0], out[1])
    }
}

/// The voice allocator: a fixed row of [`Note`] slots.
///
/// A new note takes a free slot while fewer than the limit sound. After
/// that it takes over the oldest released voice, or the oldest held one.
/// A limit of one is a monophonic instrument: a note played while another
/// is held moves that voice instead of restarting it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Notes {
    pub slots: [Note; MAX_VOICES],
    counter: u32,
    last_pitch: Option<f32>,
}

impl Default for Notes {
    fn default() -> Self {
        Self {
            slots: [Note::IDLE; MAX_VOICES],
            counter: 0,
            last_pitch: None,
        }
    }
}

impl Notes {
    pub fn latest(&self) -> Option<usize> {
        self.slots
            .iter()
            .position(|note| note.active && note.order == self.counter)
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn active(&self) -> usize {
        self.slots.iter().filter(|note| note.active).count()
    }

    /// Starts a note. `limit` is the polyphony, `glide_ms` the time a new
    /// note takes to arrive from the pitch of the one before.
    pub fn note_on(
        &mut self,
        key: u8,
        velocity: f32,
        limit: usize,
        glide_ms: f32,
        sample_rate: f32,
    ) {
        let key = key.min(127);
        let velocity = clean(velocity, 0.0, 1.0, 0.0);
        if velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        let limit = limit.clamp(1, MAX_VOICES);
        let pitch = f32::from(key);
        let glide = if glide_ms > 0.0 {
            ms_to_samples(glide_ms, sample_rate)
        } else {
            0
        };
        self.counter = self.counter.wrapping_add(1);
        if limit == 1
            && let Some(note) = self
                .slots
                .iter_mut()
                .find(|note| note.active && note.held && !note.stopping())
        {
            // Legato: the sounding voice moves to the new key.
            note.key = key;
            note.order = self.counter;
            note.release = false;
            note.retarget(pitch, glide);
            self.last_pitch = Some(pitch);
            return;
        }
        let free = if self.active() < limit {
            self.slots.iter().position(|note| !note.active)
        } else {
            None
        };
        let slot = free.unwrap_or_else(|| self.oldest());
        let old = self.slots[slot];
        let fade = ms_to_samples(FADE_MS, sample_rate);
        let mut note = Note {
            active: true,
            held: true,
            key,
            velocity,
            order: self.counter,
            pitch,
            target: pitch,
            start: true,
            ..Note::IDLE
        };
        if old.active {
            note.carry = old.last;
            note.carry_left = fade;
            note.carry_total = fade;
        }
        if glide > 0
            && let Some(from) = self.last_pitch
        {
            note.pitch = from;
            note.retarget(pitch, glide);
        }
        self.last_pitch = Some(pitch);
        self.slots[slot] = note;
    }

    /// The slot to take over: the oldest released voice, or failing that
    /// the oldest held one.
    fn oldest(&self) -> usize {
        let released = self.slots.iter().any(|note| note.active && !note.held);
        let mut slot = 0;
        let mut oldest = u32::MAX;
        for (index, note) in self.slots.iter().enumerate() {
            if note.active && (!released || !note.held) && note.order <= oldest {
                oldest = note.order;
                slot = index;
            }
        }
        slot
    }

    pub fn note_off(&mut self, key: u8) {
        let key = key.min(127);
        for note in &mut self.slots {
            if note.active && note.held && note.key == key {
                note.held = false;
                note.release = true;
            }
        }
    }

    /// Fades every voice out over a few milliseconds.
    pub fn stop(&mut self, sample_rate: f32) {
        let fade = ms_to_samples(FADE_MS, sample_rate);
        for note in &mut self.slots {
            if note.active {
                note.held = false;
                if note.fade_total == 0 {
                    note.fade_total = fade;
                    note.fade_left = fade;
                }
            }
        }
    }
}

/// A four-pole transistor-ladder lowpass, solved without a delay in its
/// feedback loop (Zavalishin, "The Art of VA Filter Design", the ladder
/// chapter). The feedback is saturated, so resonance rings and at the top
/// of its range oscillates at the cutoff without running away.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Ladder {
    s: [f32; 4],
}

impl Ladder {
    /// Filters one sample. `g` is the integrator gain of the cutoff and
    /// `k` the feedback, where 4 is the edge of self-oscillation. Returns
    /// the input node and the output of each of the four stages, which mix
    /// into the other responses (see [`ladder_low`] and its siblings).
    #[inline]
    pub fn tick(&mut self, input: f32, g: f32, k: f32) -> [f32; 5] {
        let a = g / (1.0 + g);
        let b = 1.0 - a;
        let a2 = a * a;
        let sigma = b * (a2 * a * self.s[0] + a2 * self.s[1] + a * self.s[2] + self.s[3]);
        let u = (input - k * sigma.tanh()) / (1.0 + k * a2 * a2);
        let mut out = [u, 0.0, 0.0, 0.0, 0.0];
        let mut x = u;
        for stage in 0..4 {
            let v = a * (x - self.s[stage]);
            let y = v + self.s[stage];
            self.s[stage] = flush(y + v);
            out[stage + 1] = y;
            x = y;
        }
        out
    }
}

/// Feedback of the [`Ladder`] for a resonance control from 0 to 1. The
/// top of the range is just past self-oscillation.
#[inline]
pub(crate) fn ladder_feedback(resonance: f32) -> f32 {
    resonance.clamp(0.0, 1.0) * 4.1
}

/// How much louder the ladder is driven as resonance rises, which makes up
/// part of the level the feedback takes away below the cutoff.
#[inline]
pub(crate) fn ladder_makeup(k: f32) -> f32 {
    1.0 + 0.5 * k
}

/// 24 dB per octave lowpass from the ladder's stage outputs.
#[inline]
pub(crate) fn ladder_low4(stages: &[f32; 5]) -> f32 {
    stages[4]
}

/// 12 dB per octave lowpass.
#[inline]
pub(crate) fn ladder_low2(stages: &[f32; 5]) -> f32 {
    stages[2]
}

/// 12 dB per octave bandpass with unity gain at the cutoff.
#[inline]
pub(crate) fn ladder_band2(stages: &[f32; 5]) -> f32 {
    2.0 * (stages[1] - stages[2])
}

/// 12 dB per octave highpass.
#[inline]
pub(crate) fn ladder_high2(stages: &[f32; 5]) -> f32 {
    stages[0] - 2.0 * stages[1] + stages[2]
}

/// A 12 dB per octave state-variable filter whose band state saturates,
/// the way an op-amp integrator runs out of headroom. Small signals pass
/// as through the linear filter; loud resonance is squashed and growls.
/// The update is Andrew Simper's trapezoidal form, as in
/// [`Svf`](crate::blocks::svf::Svf).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct SatSvf {
    ic1: f32,
    ic2: f32,
}

impl SatSvf {
    /// Returns the low, band and high responses. `k` is the damping, one
    /// over Q.
    #[inline]
    pub fn tick(&mut self, input: f32, g: f32, k: f32) -> (f32, f32, f32) {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = input - self.ic2;
        let v1 = a1 * self.ic1 + a2 * v3;
        let v2 = self.ic2 + a2 * self.ic1 + a3 * v3;
        self.ic1 = flush(3.0 * ((2.0 * v1 - self.ic1) / 3.0).tanh());
        self.ic2 = flush((2.0 * v2 - self.ic2).clamp(-16.0, 16.0));
        (v2, v1, input - k * v1 - v2)
    }
}

/// Damping of a state-variable filter for a resonance control from 0 to 1:
/// from no peak at all to a Q of 25.
#[inline]
pub(crate) fn svf_damping(resonance: f32) -> f32 {
    2.0 - 1.96 * resonance.clamp(0.0, 1.0)
}

/// Advances a phase by `increment` cycles. Returns the phase before the
/// step and whether the step wrapped.
#[inline]
pub(crate) fn advance(phase: &mut f32, increment: f32) -> (f32, bool) {
    let before = *phase;
    let mut next = before + increment;
    let wrapped = next >= 1.0;
    if wrapped {
        next -= 1.0;
    }
    if !(0.0..1.0).contains(&next) {
        next = 0.0;
    }
    *phase = next;
    (before, wrapped)
}

/// Where unison voice `index` of `count` sits between -1 and 1.
#[inline]
pub(crate) fn unison_position(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        2.0 * index as f32 / (count - 1) as f32 - 1.0
    }
}

/// Equal-power gains of a position from -1 (left) to 1 (right).
#[inline]
pub(crate) fn pan_gains(pan: f32) -> (f32, f32) {
    let angle = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (angle.cos(), angle.sin())
}

/// How far each slot's components are off, from -1 to 1: fixed, so every
/// render of a project is the same.
const TOLERANCE: [f32; MAX_VOICES] = [
    0.31, -0.62, 0.84, -0.17, 0.55, -0.93, 0.08, 0.72, -0.41, 0.97, -0.76, 0.23, -0.05, 0.64,
    -0.88, 0.46,
];

/// The slow pitch wander of an analogue oscillator, in cents at full
/// depth: a fixed offset per slot plus a sine no two slots share.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Drift {
    phase: f32,
}

impl Drift {
    /// Cents of detune for `slot` this sample, at most 7 either way.
    #[inline]
    pub fn tick(&mut self, slot: usize, sample_rate: f32) -> f32 {
        let slot = slot % MAX_VOICES;
        let rate_hz = 0.11 + 0.037 * slot as f32;
        let value = (TAU * (self.phase + TOLERANCE[slot] * 0.5)).sin();
        self.phase += rate_hz / sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        4.0 * value + 3.0 * TOLERANCE[slot]
    }
}

/// Blends a signal with a saturated copy of itself. At 0 it is the signal
/// unchanged; at 1 it is driven ten times as hard into a soft clip and
/// brought back to the level it had.
#[inline]
pub(crate) fn drive(input: f32, amount: f32) -> f32 {
    let amount = amount.clamp(0.0, 1.0);
    if amount <= 0.0 {
        return input;
    }
    let gain = 1.0 + 9.0 * amount;
    input + amount * ((input * gain).tanh() / gain.tanh() - input)
}

/// Seconds of one note division at a tempo.
#[inline]
pub(crate) fn division_seconds(beats: f32, bpm: f32) -> f32 {
    beats * 60.0 / bpm.clamp(20.0, 400.0)
}

/// A tempo a sequencer or delay can follow. Anything else becomes 120.
#[inline]
pub(crate) fn tempo(bpm: f32) -> f32 {
    clean(bpm, 20.0, 400.0, 120.0)
}
