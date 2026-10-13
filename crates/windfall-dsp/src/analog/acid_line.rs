use crate::blocks::adsr::Adsr;
use crate::blocks::math::clean;
use crate::blocks::oscillator::{Oscillator, Waveform, pulse};
use crate::blocks::smooth::LinearRamp;
use crate::blocks::svf::{Svf, SvfCoeffs};
use crate::instrument::Instrument;

use super::AcidLineParams;
use super::common::{Controls, increment, output, rate};

/// A monophonic resonant bass with a note-gated, sixteen-step sequencer.
/// A step's slide connects it to the following enabled step without retriggering.
pub struct AcidLine {
    sample_rate: f32,
    note_pan: crate::note_pan::NotePan,
    pan_law: windfall_core::PanLaw,
    controls: Controls<AcidLineParams>,
    oscillator: Oscillator,
    filter: Svf,
    amplitude: Adsr,
    filter_envelope: Adsr,
    pub(super) pitch: LinearRamp,
    key: u8,
    held: bool,
    velocity: f32,
    accented: bool,
    bpm: f32,
    pub(super) step: usize,
    phase: f64,
    sequencing: bool,
    gate_released: bool,
    outgoing_slide: bool,
    stop_remaining: Option<u32>,
}

impl Default for AcidLine {
    fn default() -> Self {
        Self {
            sample_rate: 48_000.0,
            note_pan: crate::note_pan::NotePan::default(),
            pan_law: windfall_core::PanLaw::Balance,
            controls: Controls::default(),
            oscillator: Oscillator::default(),
            filter: Svf::default(),
            amplitude: Adsr::default(),
            filter_envelope: Adsr::default(),
            pitch: LinearRamp::new(36.0),
            key: 36,
            held: false,
            velocity: 0.0,
            accented: false,
            bpm: 120.0,
            step: 0,
            phase: 0.0,
            sequencing: false,
            gate_released: false,
            outgoing_slide: false,
            stop_remaining: None,
        }
    }
}

impl AcidLine {
    fn trigger(&mut self, pitch: f32, accent: bool, slide: bool) {
        let p = self.controls.current;
        if slide && !self.amplitude.is_idle() {
            self.pitch.set_target(
                pitch,
                (p.slide_ms * 0.001 * self.sample_rate).round() as u32,
            );
        } else {
            self.pitch.snap(pitch);
            self.amplitude.gate_on();
            self.filter_envelope.gate_on();
        }
        self.accented = accent;
    }
    fn release(&mut self) {
        self.amplitude.gate_off();
        self.filter_envelope.gate_off();
        self.gate_released = true;
    }
    fn start_step(&mut self, connected: bool) {
        let step = self.controls.current.steps[self.step];
        self.gate_released = false;
        self.outgoing_slide = step.enabled && step.slide;
        if step.enabled {
            self.trigger(
                (self.key as f32 + step.pitch_offset as f32).clamp(0.0, 127.0),
                step.accent,
                connected,
            );
        } else {
            self.release();
        }
    }
}

impl Instrument for AcidLine {
    type Params = AcidLineParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = rate(sample_rate);
        self.controls.prepare(self.sample_rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.note_pan = crate::note_pan::NotePan::new(0.0, self.pan_law);
        self.controls.reset();
        self.oscillator = Oscillator::default();
        self.filter.reset();
        self.amplitude.reset();
        self.filter_envelope.reset();
        self.pitch.snap(36.0);
        self.key = 36;
        self.held = false;
        self.velocity = 0.0;
        self.accented = false;
        self.step = 0;
        self.phase = 0.0;
        self.sequencing = false;
        self.gate_released = false;
        self.outgoing_slide = false;
        self.stop_remaining = None;
    }
    fn set_params(&mut self, params: &Self::Params) {
        let was_sequencing = self.controls.current.sequencer;
        self.controls.set(params);
        let sequencing = self.controls.current.sequencer;
        if sequencing != was_sequencing && self.held {
            self.phase = 0.0;
            self.step = 0;
            self.sequencing = sequencing;
            if sequencing {
                self.start_step(false);
            } else {
                self.trigger(self.key.min(127) as f32, self.velocity >= 0.8, false);
            }
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.bpm = clean(bpm, 20.0, 400.0, 120.0);
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        let velocity = clean(velocity, 0.0, 1.0, 0.0);
        if velocity <= 0.0 {
            self.note_off(key);
            return;
        }
        self.note_pan = crate::note_pan::NotePan::new(0.0, self.pan_law);
        let slide = self.held && self.controls.current.manual_slide;
        self.key = key;
        self.velocity = velocity;
        self.held = true;
        self.stop_remaining = None;
        self.sequencing = self.controls.current.sequencer;
        if self.sequencing {
            self.step = 0;
            self.phase = 0.0;
            self.start_step(false);
        } else {
            self.trigger(key.min(127) as f32, velocity >= 0.8, slide);
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
        // Last-note priority: an older key's release never releases the new key.
        if self.held && self.key == key {
            self.held = false;
            self.sequencing = false;
            self.release();
        }
    }
    fn all_notes_off(&mut self) {
        self.held = false;
        self.sequencing = false;
        if !self.amplitude.is_idle() {
            self.stop_remaining = Some((self.sample_rate * 0.005).round().max(1.0) as u32);
        }
    }
    fn active_voices(&self) -> usize {
        usize::from(!self.amplitude.is_idle() || (self.sequencing && self.held))
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (left, right) in left.iter_mut().zip(right) {
            let p = self.controls.tick();
            if self.sequencing && self.held {
                if self.phase + 1.0e-12 >= 1.0 {
                    self.phase = (self.phase - 1.0).max(0.0);
                    self.step = (self.step + 1) % 16;
                    self.start_step(self.outgoing_slide);
                }
                if !self.gate_released && !self.outgoing_slide && self.phase >= p.gate as f64 {
                    self.release();
                }
                // One sixteenth note is a quarter beat. Preserve phase on tempo edits.
                self.phase += self.bpm as f64 / (15.0 * self.sample_rate as f64);
            }
            if self.amplitude.is_idle() {
                self.filter.reset();
                *left = 0.0;
                *right = 0.0;
                continue;
            }
            self.amplitude
                .configure(2.0, p.decay_ms, 0.6, 45.0, self.sample_rate);
            self.filter_envelope
                .configure(1.0, p.decay_ms, 0.0, 45.0, self.sample_rate);
            let amplitude = self.amplitude.tick();
            let envelope = self.filter_envelope.tick();
            let inc = increment(self.pitch.tick(), self.sample_rate);
            let pulse = pulse(self.oscillator.phase(), inc, 0.5);
            let saw = self.oscillator.tick(Waveform::Saw, inc, 0.5);
            let accent = if self.accented { p.accent_amount } else { 0.0 };
            let cutoff = p.cutoff_hz * (envelope * p.envelope_amount * 5.0 + accent * 1.5).exp2();
            let coeffs = SvfCoeffs::new(cutoff, 0.707 + p.resonance * 11.0, self.sample_rate);
            let raw = saw + (pulse - saw) * p.pulse_mix;
            let mut value = self.filter.tick(&coeffs, raw.tanh()).low
                * amplitude
                * self.velocity
                * (1.0 + accent)
                * p.gain
                * 0.5;
            if let Some(remaining) = self.stop_remaining {
                value *= remaining as f32 / (self.sample_rate * 0.005).round().max(1.0);
                if remaining <= 1 {
                    self.amplitude.reset();
                    self.filter_envelope.reset();
                    self.filter.reset();
                    self.stop_remaining = None;
                    value = 0.0;
                } else {
                    self.stop_remaining = Some(remaining - 1);
                }
            }
            if self.amplitude.is_idle() {
                value = 0.0;
            }
            let pan = self.note_pan.tick();
            let sample = output(value);
            *left = sample * pan.0;
            *right = sample * pan.1;
        }
    }
}
