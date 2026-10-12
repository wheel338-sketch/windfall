use crate::ParamSet;
use crate::blocks::math::clean;

pub(super) type Frame = [f32; 2];

/// Exact-size storage: DelayLine's power-of-two guard would exceed our 2 s bound.
#[derive(Default)]
pub(super) struct Ring {
    pub data: Vec<Frame>,
    pub write: usize,
    pub filled: usize,
}
impl Ring {
    pub fn prepare(&mut self, rate: f32) {
        self.data = vec![[0.0; 2]; (rate as f64 * 2.0).floor().max(2.0) as usize];
        self.clear();
    }
    pub fn clear(&mut self) {
        // Old PCM is unreachable until overwritten. Reset is constant-time,
        // even at 384 kHz; read masks every not-yet-written physical slot.
        self.write = 0;
        self.filled = 0;
    }
    pub fn push(&mut self, frame: Frame) {
        if self.data.is_empty() {
            return;
        }
        self.data[self.write] = frame.map(finite);
        self.write = (self.write + 1) % self.data.len();
        self.filled = (self.filled + 1).min(self.data.len());
    }
    pub fn start(&self, length: usize) -> usize {
        if self.data.is_empty() {
            0
        } else {
            (self.write + self.data.len() - length.min(self.data.len())) % self.data.len()
        }
    }
    /// Linear interpolation wraps INSIDE the slice, including its last neighbor.
    pub fn read(&self, start: usize, length: usize, position: f64) -> Frame {
        if self.data.is_empty() {
            return [0.0; 2];
        }
        let length = length.clamp(1, self.data.len());
        let position = position.rem_euclid(length as f64);
        let index = position as usize;
        let t = position - index as f64;
        let read = |index: usize| {
            let index = index % self.data.len();
            let age = (self.write + self.data.len() - index - 1) % self.data.len();
            if age < self.filled { self.data[index] } else { [0.0; 2] }
        };
        let a = read(start + index);
        let b = read(start + (index + 1) % length);
        std::array::from_fn(|c| finite((a[c] as f64 * (1.0 - t) + b[c] as f64 * t) as f32))
    }
}

pub(super) fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0e6, 1.0e6)
    } else {
        0.0
    }
}
pub(super) fn blend(a: Frame, b: Frame, mix: f32) -> Frame {
    std::array::from_fn(|c| finite(a[c] * (1.0 - mix) + b[c] * mix))
}

/// Per-frame parameter interpolation; timing follows target beats immediately.
pub(super) struct Controls<P> {
    pub current: P,
    pub target: P,
    first: bool,
    coefficient: f32,
}
impl<P: ParamSet> Default for Controls<P> {
    fn default() -> Self {
        Self {
            current: P::default(),
            target: P::default(),
            first: true,
            coefficient: 1.0,
        }
    }
}
impl<P: ParamSet> Controls<P> {
    pub fn prepare(&mut self, rate: f32) {
        self.coefficient = crate::blocks::math::smoothing_coefficient(5.0, rate);
        self.reset();
    }
    pub fn reset(&mut self) {
        self.current = self.target;
        self.first = true;
    }
    pub fn set(&mut self, value: &P) {
        self.target = value.sanitized();
        if self.first {
            self.current = self.target;
        }
    }
    pub fn tick(&mut self) -> P {
        self.first = false;
        self.current.approach(&self.target, self.coefficient);
        self.current
    }
}

/// Short transition from the last emitted wet sample at deliberate head jumps.
#[derive(Default)]
pub(super) struct JumpFade {
    last: Frame,
    from: Frame,
    remaining: usize,
    length: usize,
}
impl JumpFade {
    pub fn prepare(&mut self, rate: f32) {
        self.length = (rate * 0.002).round().max(1.0) as usize;
        self.reset();
    }
    pub fn reset(&mut self) {
        self.last = [0.0; 2];
        self.from = self.last;
        self.remaining = 0;
    }
    pub fn jump(&mut self) {
        self.from = self.last;
        self.remaining = self.length;
    }
    pub fn tick(&mut self, next: Frame) -> Frame {
        self.last = if self.remaining > 0 {
            self.remaining -= 1;
            blend(
                self.from,
                next,
                1.0 - self.remaining as f32 / self.length as f32,
            )
        } else {
            next.map(finite)
        };
        self.last
    }
}

pub(super) fn sample_rate(rate: f32) -> f32 {
    clean(rate, 1.0, 384_000.0, 48_000.0)
}
pub(super) fn tempo(bpm: f32) -> f32 {
    clean(bpm, 20.0, 400.0, 120.0)
}
pub(super) fn loop_length(rate: f32, bpm: f32, beats: f32, capacity: usize) -> usize {
    (rate as f64 * 60.0 / bpm as f64 * beats as f64)
        .round()
        .max(1.0)
        .min(capacity.max(1) as f64) as usize
}
