use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::{clean, flush, ms_to_samples};
use crate::blocks::smooth::LinearRamp;

pub(super) fn rate(value: f32) -> f32 {
    clean(value, 1.0, 384_000.0, 48_000.0)
}
pub(super) fn audio(value: f32) -> f32 {
    flush(clean(value, -1_000.0, 1_000.0, 0.0))
}
pub(super) fn bounded(value: f32) -> f32 {
    flush(clean(value, -1.0e6, 1.0e6, 0.0))
}
pub(super) fn frames(ms: f32, rate: f32) -> usize {
    (f64::from(ms) * 0.001 * f64::from(rate)).ceil().max(1.0) as usize
}
pub(super) fn samples(ms: f32, rate: f32) -> f32 {
    (f64::from(ms) * 0.001 * f64::from(rate)) as f32
}
pub(super) fn tap(line: &DelayLine, delay: f32) -> f32 {
    line.tap_linear(delay.clamp(1.0, line.max_delay().max(1) as f32))
}
pub(super) fn sine(phase: f64) -> f32 {
    (std::f64::consts::TAU * phase).sin() as f32
}
pub(super) fn advance(phase: &mut f64, hz: f32, rate: f32) {
    *phase = (*phase + f64::from(hz) / f64::from(rate)).fract();
}

/// One cycle in `beats` quarter notes, without resetting the running phase.
/// Free rates retain the legacy arithmetic. Host tempo is bounded consistently
/// with the delay bank; invalid tempo falls back to 120 BPM.
pub(super) fn modulation_hz(free: f32, sync: bool, beats: f32, bpm: f32) -> f32 {
    if sync {
        clean(bpm, 10.0, 1000.0, 120.0) / (60.0 * beats)
    } else {
        free
    }
}

/// Fixed 10 ms ramps, including topology weights. Writes before the first
/// processed frame snap. Repeated equal targets never restart a ramp.
pub(super) struct Controls<const N: usize> {
    ramps: [LinearRamp; N],
    length: u32,
    fresh: bool,
}
impl<const N: usize> Default for Controls<N> {
    fn default() -> Self {
        Self {
            ramps: [LinearRamp::new(0.0); N],
            length: 480,
            fresh: true,
        }
    }
}
impl<const N: usize> Controls<N> {
    pub fn prepare(&mut self, rate: f32) {
        self.length = ms_to_samples(10.0, rate);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.fresh = true;
    }
    pub fn set(&mut self, values: [f32; N]) {
        for (ramp, value) in self.ramps.iter_mut().zip(values) {
            ramp.set_target(value, if self.fresh { 0 } else { self.length });
        }
    }
    pub fn tick(&mut self) -> [f32; N] {
        self.fresh = false;
        std::array::from_fn(|i| self.ramps[i].tick())
    }
    /// Length of a ramp started now: nothing before the first frame.
    pub fn glide(&self) -> u32 {
        if self.fresh { 0 } else { self.length }
    }
}

/// One cycle of a triangle that starts at 0, peaks at 1 half way and is
/// back at 0 at the end.
pub(super) fn triangle(phase: f64) -> f32 {
    (1.0 - (1.0 - 2.0 * phase.fract()).abs()) as f32
}

#[derive(Default, Clone, Copy)]
pub(super) struct Lowpass {
    state: f32,
}
impl Lowpass {
    pub fn tick(&mut self, input: f32, pole: f32) -> f32 {
        self.state = bounded((1.0 - pole) * input + pole * self.state);
        self.state
    }
    pub fn clear(&mut self) {
        self.state = 0.0;
    }
}
pub(super) fn pole(hz: f32, rate: f32) -> f32 {
    (-std::f32::consts::TAU * hz.min(rate * 0.45) / rate)
        .exp()
        .min(0.9999)
}

/// An explicit silence lifetime, not an RT60 approximation. The last 10 ms
/// fade to zero; callers clear recursive histories at expiry without freeing
/// them. New audio during the fade recovers smoothly. Thus tail_samples is a
/// true maximum under automation too, even for a modulated IIR network.
pub(super) struct Tail {
    idle: usize,
    hold: usize,
    fade: usize,
    gain: LinearRamp,
}
impl Default for Tail {
    fn default() -> Self {
        Self {
            idle: 0,
            hold: 48_000,
            fade: 480,
            gain: LinearRamp::new(1.0),
        }
    }
}
impl Tail {
    pub fn prepare(&mut self, seconds: f32, rate: f32) {
        self.hold = (seconds * rate).ceil() as usize;
        self.fade = frames(10.0, rate);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.idle = 0;
        self.gain.snap(1.0);
    }
    pub fn samples(&self) -> usize {
        self.hold + self.fade
    }
    pub fn tick(&mut self, input: [f32; 2]) -> (f32, bool) {
        let was_expired = self.idle == self.samples();
        if input.iter().any(|x| x.abs() >= 1.0e-20) {
            self.idle = 0;
            self.gain.set_target(1.0, self.fade as u32);
        } else {
            self.idle = (self.idle + 1).min(self.samples());
            if self.idle > self.hold {
                self.gain
                    .snap(1.0 - (self.idle - self.hold) as f32 / self.fade as f32);
            }
        }
        (
            self.gain.tick(),
            !was_expired && self.idle == self.samples(),
        )
    }
}
