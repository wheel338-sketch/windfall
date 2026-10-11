//! The small effects built into the instruments of this family: a
//! bucket-brigade style chorus that also serves as a flanger, a feedback
//! delay, a phaser and a room reverb.
//!
//! Each allocates its lines in `prepare` and nothing afterwards. With its
//! mix at zero each returns its input unchanged, and once its input is
//! silent each one's output reaches exact silence.

use std::f32::consts::{PI, TAU};

use crate::blocks::delay_line::DelayLine;
use crate::blocks::math::{flush, smoothing_coefficient};

/// The longest echo time of [`StereoDelay`], in seconds.
pub(crate) const MAX_DELAY_SECONDS: f32 = 1.5;

/// Zeroes a feedback sample once it is far below hearing, so a tail ends
/// in exact silence instead of fading for minutes.
#[inline]
fn quiet(value: f32) -> f32 {
    if value.abs() < 1.0e-7 { 0.0 } else { value }
}

#[inline]
fn slew(current: f32, target: f32, coefficient: f32) -> f32 {
    let gap = target - current;
    if gap.abs() <= 1.0e-6 * (1.0 + target.abs()) {
        target
    } else {
        current + gap * coefficient
    }
}

/// A conservative -180 dB decay bound, with headroom for summed voices/taps.
fn decay_repeats(feedback: f32) -> usize {
    if feedback <= 0.0 {
        1
    } else {
        (1.0e-9_f32.ln() / feedback.clamp(0.001, 0.95).ln()).ceil() as usize + 1
    }
}

/// Two modulated delay taps, one a side, swept in opposite directions by a
/// triangle: the stereo chorus of a string machine. With a short centre
/// delay and feedback it is a flanger.
pub(crate) struct Chorus {
    lines: [DelayLine; 2],
    phase: f32,
    centre: f32,
    swing: f32,
    mix: f32,
    last: [f32; 2],
    sample_rate: f32,
    coefficient: f32,
}

impl Default for Chorus {
    fn default() -> Self {
        Self {
            lines: [DelayLine::default(), DelayLine::default()],
            phase: 0.0,
            centre: 0.0,
            swing: 0.0,
            mix: 0.0,
            last: [0.0; 2],
            sample_rate: 48_000.0,
            coefficient: 1.0,
        }
    }
}

impl Chorus {
    /// Longest tap, in seconds.
    const SPAN: f32 = 0.04;

    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        let length = (Self::SPAN * sample_rate) as usize + 8;
        self.lines = [DelayLine::new(length), DelayLine::new(length)];
        self.coefficient = smoothing_coefficient(20.0, sample_rate);
        self.reset();
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.phase = 0.0;
        self.centre = 0.0;
        self.swing = 0.0;
        self.mix = 0.0;
        self.last = [0.0; 2];
    }

    /// Samples after silent input for which output can continue.
    pub fn tail_samples(&self) -> usize {
        (Self::SPAN * self.sample_rate) as usize * 8
    }

    /// The chorus proper: an 8 ms tap swept by up to 4 ms.
    #[inline]
    pub fn tick(
        &mut self,
        left: f32,
        right: f32,
        rate_hz: f32,
        depth: f32,
        mix: f32,
    ) -> (f32, f32) {
        self.tick_shaped(left, right, rate_hz, 8.0, 4.0 * depth, 0.0, mix)
    }

    /// One sample through taps centred on `centre_ms` and swept by
    /// `swing_ms` either way, with `feedback` of each tap returned to its
    /// own line.
    #[allow(clippy::too_many_arguments)] // One value per control of the effect.
    #[inline]
    pub fn tick_shaped(
        &mut self,
        left: f32,
        right: f32,
        rate_hz: f32,
        centre_ms: f32,
        swing_ms: f32,
        feedback: f32,
        mix: f32,
    ) -> (f32, f32) {
        let top = Self::SPAN * self.sample_rate - 4.0;
        let centre = (centre_ms * 0.001 * self.sample_rate).clamp(4.0, top);
        let swing = (swing_ms.max(0.0) * 0.001 * self.sample_rate).min(centre - 3.0);
        if self.centre == 0.0 {
            self.centre = centre;
            self.swing = swing;
            self.mix = mix;
        } else {
            self.centre = slew(self.centre, centre, self.coefficient);
            self.swing = slew(self.swing, swing, self.coefficient);
            self.mix = slew(self.mix, mix, self.coefficient);
        }
        let triangle = 4.0 * (self.phase - 0.5).abs() - 1.0;
        self.phase += (rate_hz / self.sample_rate).clamp(0.0, 0.5);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let swing = self.swing.min(self.centre - 3.0).max(0.0);
        let wet_left = self.lines[0].tap_cubic((self.centre + swing * triangle).clamp(3.0, top));
        let wet_right = self.lines[1].tap_cubic((self.centre - swing * triangle).clamp(3.0, top));
        let feedback = feedback.clamp(-0.9, 0.9);
        self.lines[0].push(quiet(left + feedback * self.last[0]));
        self.lines[1].push(quiet(right + feedback * self.last[1]));
        self.last = [flush(wet_left), flush(wet_right)];
        (left + self.mix * wet_left, right + self.mix * wet_right)
    }
}

/// Echoes with feedback, a side each or bouncing between the sides. The
/// repeats lose a little treble every time round.
pub(crate) struct StereoDelay {
    lines: [DelayLine; 2],
    time: f32,
    damp: [f32; 2],
    max: f32,
    coefficient: f32,
}

impl Default for StereoDelay {
    fn default() -> Self {
        Self {
            lines: [DelayLine::default(), DelayLine::default()],
            time: 0.0,
            damp: [0.0; 2],
            max: 4.0,
            coefficient: 1.0,
        }
    }
}

impl StereoDelay {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.max = MAX_DELAY_SECONDS * sample_rate;
        let length = self.max as usize + 8;
        self.lines = [DelayLine::new(length), DelayLine::new(length)];
        self.coefficient = smoothing_coefficient(60.0, sample_rate);
        self.reset();
    }

    pub fn reset(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.time = 0.0;
        self.damp = [0.0; 2];
    }

    /// Covers feedback repeats, not just the first arrival. A changed tempo
    /// may leave the tap at its previous time, so use the maximum line length.
    pub fn tail_samples(&self, feedback: f32) -> usize {
        (self.max as usize + 8) * decay_repeats(feedback)
    }

    /// One sample. `delay_samples` is the echo time, `feedback` the share
    /// of each echo that makes the next, at most 0.9.
    #[inline]
    pub fn tick(
        &mut self,
        left: f32,
        right: f32,
        delay_samples: f32,
        feedback: f32,
        mix: f32,
        ping_pong: bool,
    ) -> (f32, f32) {
        let target = if delay_samples.is_finite() {
            delay_samples.clamp(2.0, self.max.max(2.0))
        } else {
            2.0
        };
        self.time = if self.time < 2.0 {
            target
        } else {
            slew(self.time, target, self.coefficient)
        };
        let wet_left = self.lines[0].tap_linear(self.time);
        let wet_right = self.lines[1].tap_linear(self.time);
        self.damp[0] = flush(self.damp[0] + 0.65 * (wet_left - self.damp[0]));
        self.damp[1] = flush(self.damp[1] + 0.65 * (wet_right - self.damp[1]));
        let feedback = feedback.clamp(0.0, 0.9);
        let (into_left, into_right) = if ping_pong {
            (
                0.5 * (left + right) + feedback * self.damp[1],
                feedback * self.damp[0],
            )
        } else {
            (
                left + feedback * self.damp[0],
                right + feedback * self.damp[1],
            )
        };
        self.lines[0].push(quiet(into_left));
        self.lines[1].push(quiet(into_right));
        (left + mix * wet_left, right + mix * wet_right)
    }
}

/// Four first-order allpass stages a side, swept together by a sine. Mixed
/// with the dry signal they cut two moving notches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Phaser {
    state: [[f32; 4]; 2],
    phase: f32,
    sample_rate: f32,
}

impl Default for Phaser {
    fn default() -> Self {
        Self {
            state: [[0.0; 4]; 2],
            phase: 0.0,
            sample_rate: 48_000.0,
        }
    }
}

impl Phaser {
    pub fn prepare(&mut self, sample_rate: f32) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.state = [[0.0; 4]; 2];
        self.phase = 0.0;
    }

    #[inline]
    pub fn tick(
        &mut self,
        left: f32,
        right: f32,
        rate_hz: f32,
        depth: f32,
        mix: f32,
    ) -> (f32, f32) {
        let sweep = (TAU * self.phase).sin();
        self.phase += (rate_hz / self.sample_rate).clamp(0.0, 0.5);
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        let centre =
            (700.0 * (2.0 * depth.clamp(0.0, 1.0) * sweep).exp2()).min(self.sample_rate * 0.4);
        let tangent = (PI * centre / self.sample_rate).tan();
        let a = (tangent - 1.0) / (tangent + 1.0);
        let mut out = [left, right];
        for (side, value) in out.iter_mut().enumerate() {
            let mut x = *value;
            for stage in &mut self.state[side] {
                let y = a * x + *stage;
                *stage = flush(x - a * y);
                x = y;
            }
            *value += 0.5 * mix * (x - *value);
        }
        (out[0], out[1])
    }
}

/// Comb lengths of the reverb in samples at 44.1 kHz, the right side a
/// little longer than the left. These are the mutually prime lengths
/// Schroeder's design calls for ("Natural sounding artificial
/// reverberation", JAES 1962).
const COMBS: [usize; 4] = [1116, 1277, 1422, 1617];
const ALLPASSES: [usize; 2] = [556, 225];
const STEREO_SPREAD: usize = 23;

/// A small room: four damped feedback combs into two allpasses, a side.
pub(crate) struct Reverb {
    combs: [[DelayLine; 4]; 2],
    comb_lengths: [[usize; 4]; 2],
    damp: [[f32; 4]; 2],
    allpasses: [[DelayLine; 2]; 2],
    allpass_lengths: [[usize; 2]; 2],
    longest: usize,
}

impl Default for Reverb {
    fn default() -> Self {
        Self {
            combs: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::default())),
            comb_lengths: [[1; 4]; 2],
            damp: [[0.0; 4]; 2],
            allpasses: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::default())),
            allpass_lengths: [[1; 2]; 2],
            longest: 1,
        }
    }
}

impl Reverb {
    pub fn prepare(&mut self, sample_rate: f32) {
        let scale = sample_rate / 44_100.0;
        let scaled = |length: usize, side: usize| {
            (((length + side * STEREO_SPREAD) as f32 * scale).round() as usize).max(1)
        };
        self.longest = 1;
        for side in 0..2 {
            for (index, &length) in COMBS.iter().enumerate() {
                let length = scaled(length, side);
                self.comb_lengths[side][index] = length;
                self.combs[side][index] = DelayLine::new(length + 1);
                self.longest = self.longest.max(length);
            }
            for (index, &length) in ALLPASSES.iter().enumerate() {
                let length = scaled(length, side);
                self.allpass_lengths[side][index] = length;
                self.allpasses[side][index] = DelayLine::new(length + 1);
            }
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for side in 0..2 {
            for line in &mut self.combs[side] {
                line.clear();
            }
            for line in &mut self.allpasses[side] {
                line.clear();
            }
        }
        self.damp = [[0.0; 4]; 2];
    }

    /// Comb decay plus the two serial allpass decays. Runtime adds the other
    /// effects' bounds because the room receives their delayed outputs.
    pub fn tail_samples(&self, size: f32) -> usize {
        let feedback = 0.7 + 0.25 * size.clamp(0.0, 1.0);
        self.longest * decay_repeats(feedback)
            + self.allpass_lengths[1].iter().sum::<usize>() * decay_repeats(0.5)
    }

    /// One sample. `size` from 0 to 1 sets how long the room rings.
    #[inline]
    pub fn tick(&mut self, left: f32, right: f32, size: f32, mix: f32) -> (f32, f32) {
        let feedback = 0.7 + 0.25 * size.clamp(0.0, 1.0);
        let input = 0.06 * (left + right);
        let mut wet = [0.0_f32; 2];
        for (side, wet) in wet.iter_mut().enumerate() {
            let mut sum = 0.0;
            for index in 0..4 {
                let delayed = self.combs[side][index].tap(self.comb_lengths[side][index]);
                let damped = flush(delayed + 0.3 * (self.damp[side][index] - delayed));
                self.damp[side][index] = damped;
                self.combs[side][index].push(quiet(input + feedback * damped));
                sum += delayed;
            }
            for index in 0..2 {
                let delayed = self.allpasses[side][index].tap(self.allpass_lengths[side][index]);
                self.allpasses[side][index].push(quiet(sum + 0.5 * delayed));
                sum = delayed - 0.5 * sum;
            }
            *wet = sum;
        }
        (left + mix * wet[0], right + mix * wet[1])
    }
}
