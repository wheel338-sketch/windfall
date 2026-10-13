//! Band-limited oscillator waveforms.
//!
//! A saw or a pulse drawn naively has corners sharper than the sample rate
//! can represent, and the excess folds back as inharmonic "aliasing" tones.
//! The waveforms here are drawn naively and then corrected around each jump
//! with a polynomial that rounds it by exactly the right amount (PolyBLEP).
//! The polynomial is the four-point one derived from a cubic B-spline, which
//! suppresses aliasing far better than the common two-point version
//! (Välimäki, Pekonen and Nam, "Perceptually informed synthesis of
//! bandlimited classical waveforms using integrated polynomial
//! interpolation", JASA 2012). The triangle's corners get the integral of
//! that polynomial (PolyBLAMP; Esqueda, Välimäki and Bilbao, "Rounding
//! corners with BLAMP", DAFx 2016).

use std::f32::consts::TAU;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The waveform an oscillator plays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Waveform {
    /// A pure tone with no overtones.
    Sine,
    /// Soft and hollow: odd overtones that fall away quickly.
    Triangle,
    /// Bright and buzzy: every overtone.
    #[default]
    Saw,
    /// Hollow and woody: odd overtones only.
    Square,
    /// A square whose two halves can differ in length, set by the pulse
    /// width. Narrow pulses sound thin and nasal.
    Pulse,
    /// Hiss with equal energy at every frequency. It has no pitch.
    WhiteNoise,
    /// Softer hiss with equal energy in every octave. It has no pitch.
    PinkNoise,
}

impl Waveform {
    /// True for the two noise colours, which have no phase and are made by
    /// [`Rng`](super::noise::Rng) and [`PinkNoise`](super::noise::PinkNoise)
    /// instead of [`tonal_sample`].
    pub fn is_noise(self) -> bool {
        matches!(self, Waveform::WhiteNoise | Waveform::PinkNoise)
    }
}

/// What to add to a naive upward unit step so that it is band-limited.
/// `t` is the time since the step in samples, from -2 to 2; negative values
/// are samples before the step.
#[inline]
pub fn blep(t: f32) -> f32 {
    if t < -1.0 {
        let u = 2.0 + t;
        let u2 = u * u;
        u2 * u2 * (1.0 / 24.0)
    } else if t < 0.0 {
        let t2 = t * t;
        0.5 + t * (2.0 / 3.0) - t2 * t * (1.0 / 3.0) - t2 * t2 * 0.125
    } else if t < 1.0 {
        let t2 = t * t;
        -0.5 + t * (2.0 / 3.0) - t2 * t * (1.0 / 3.0) + t2 * t2 * 0.125
    } else {
        let u = 2.0 - t;
        let u2 = u * u;
        -u2 * u2 * (1.0 / 24.0)
    }
}

/// What to add to a naive corner whose slope rises by one per sample so
/// that it is band-limited. `t` is the time since the corner in samples,
/// from -2 to 2.
#[inline]
pub fn blamp(t: f32) -> f32 {
    let t = t.abs();
    if t < 1.0 {
        let t2 = t * t;
        7.0 / 30.0 - 0.5 * t + t2 * (1.0 / 3.0) - t2 * t2 * (1.0 / 12.0) + t2 * t2 * t * 0.025
    } else {
        let u = 2.0 - t;
        let u2 = u * u;
        u2 * u2 * u * (1.0 / 120.0)
    }
}

/// The correction for a feature at phase zero, seen from `phase`, when the
/// phase advances by `increment` per sample. The feature reaches two samples
/// to either side.
#[inline]
fn around_zero(phase: f32, increment: f32, residual: impl Fn(f32) -> f32) -> f32 {
    let reach = 2.0 * increment;
    if phase < reach {
        residual(phase / increment)
    } else if phase > 1.0 - reach {
        residual((phase - 1.0) / increment)
    } else {
        0.0
    }
}

/// Moves a phase back by `offset` cycles, wrapping into 0..1.
#[inline]
fn shifted(phase: f32, offset: f32) -> f32 {
    let moved = phase - offset;
    if moved < 0.0 { moved + 1.0 } else { moved }
}

/// A sine at `phase`, which runs from 0 to 1 over one cycle.
#[inline]
pub fn sine(phase: f32) -> f32 {
    (TAU * phase).sin()
}

/// The additive bank's bounded minimax sine, for dense operator kernels.
/// Fold to a quarter-cycle before the degree-seven odd polynomial; the
/// independent standard-library curve remains `sine` above.
#[inline(always)]
pub(crate) fn sine_fast(phase: f32) -> f32 {
    let (sign, quarter) = if phase < 0.5 {
        (1.0, phase)
    } else {
        (-1.0, phase - 0.5)
    };
    let quarter = if quarter > 0.25 {
        0.5 - quarter
    } else {
        quarter
    };
    let y = TAU * quarter;
    let square = y * y;
    sign * y * (1.0 + square * (-0.166_656_8 + square * (0.008_312_37 + square * -0.000_184_92)))
}

/// A saw that rises from -1 to 1 over the cycle. `increment` is the phase
/// step per sample, which is the frequency divided by the sample rate; it
/// must stay below 0.25.
#[inline]
pub fn saw(phase: f32, increment: f32) -> f32 {
    2.0 * phase - 1.0 - 2.0 * around_zero(phase, increment, blep)
}

/// A pulse that is high for the first `width` of the cycle, 0 to 1, and low
/// for the rest. Its average is removed, so changing the width does not
/// shift the waveform up or down.
#[inline]
pub fn pulse(phase: f32, increment: f32, width: f32) -> f32 {
    let naive = if phase < width { 1.0 } else { -1.0 };
    let rise = around_zero(phase, increment, blep);
    let fall = around_zero(shifted(phase, width), increment, blep);
    naive + 2.0 * (rise - fall) - (2.0 * width - 1.0)
}

/// A triangle that starts at its lowest point.
#[inline]
pub fn triangle(phase: f32, increment: f32) -> f32 {
    let naive = if phase < 0.5 {
        4.0 * phase - 1.0
    } else {
        3.0 - 4.0 * phase
    };
    // The slope jumps by 8 per cycle at each corner: up at the trough, down
    // at the peak.
    let trough = around_zero(phase, increment, blamp);
    let peak = around_zero(shifted(phase, 0.5), increment, blamp);
    naive + 8.0 * increment * (trough - peak)
}

/// One sample of a pitched waveform at `phase`. The two noise waveforms
/// return silence here.
#[inline]
pub fn tonal_sample(waveform: Waveform, phase: f32, increment: f32, width: f32) -> f32 {
    match waveform {
        Waveform::Sine => sine(phase),
        Waveform::Triangle => triangle(phase, increment),
        Waveform::Saw => saw(phase, increment),
        Waveform::Square => pulse(phase, increment, 0.5),
        Waveform::Pulse => pulse(phase, increment, width),
        Waveform::WhiteNoise | Waveform::PinkNoise => 0.0,
    }
}

/// Highest phase step per sample the corrections are valid for.
pub const MAX_INCREMENT: f32 = 0.249;

/// A phase that advances and wraps: the simplest way to play one of the
/// waveforms above.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Oscillator {
    phase: f32,
}

impl Oscillator {
    /// An oscillator starting at `phase`, 0 to 1.
    pub fn new(phase: f32) -> Self {
        Self {
            phase: phase - phase.floor(),
        }
    }

    #[inline]
    pub fn phase(&self) -> f32 {
        self.phase
    }

    /// Returns the sample at the current phase, then advances by
    /// `increment` cycles (frequency divided by sample rate).
    #[inline]
    pub fn tick(&mut self, waveform: Waveform, increment: f32, width: f32) -> f32 {
        let increment = increment.clamp(0.0, MAX_INCREMENT);
        let value = tonal_sample(waveform, self.phase, increment, width);
        self.advance(increment);
        value
    }

    #[inline]
    pub(crate) fn advance(&mut self, increment: f32) {
        self.advance_bounded(increment.clamp(0.0, MAX_INCREMENT));
    }

    /// Prepared operator increments have already been clamped to this range.
    /// Keep the generic advance's clamp at its other call sites, and avoid
    /// repeating that sanitation for both substeps of each matrix operator.
    #[inline]
    pub(crate) fn advance_bounded(&mut self, increment: f32) {
        self.phase += increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;
    #[test]
    fn bounded_prepared_increments_preserve_generic_phase_bits() {
        for i in 0..1024 {
            let increment = i as f32 / 1023.0 * MAX_INCREMENT;
            let mut generic = Oscillator::new(i as f32 * 0.137);
            let mut prepared = generic;
            for _ in 0..1024 {
                generic.advance(increment);
                prepared.advance_bounded(increment);
                assert_eq!(generic.phase().to_bits(), prepared.phase().to_bits());
            }
        }
    }

    #[test]
    fn dense_sine_polynomial_is_bounded_against_independent_trigonometry() {
        let mut error = 0.0_f64;
        for n in 0..=1_000_000 {
            let phase = n as f32 / 1_000_000.0;
            let reference = (std::f64::consts::TAU * f64::from(phase)).sin();
            error = error.max((f64::from(sine_fast(phase)) - reference).abs());
        }
        assert!(error < 1.5e-6, "sine worst error {error}");
    }

    fn render(waveform: Waveform, frequency: f64, width: f32, samples: usize) -> Vec<f32> {
        let mut oscillator = Oscillator::default();
        (0..samples)
            .map(|_| oscillator.tick(waveform, (frequency / RATE) as f32, width))
            .collect()
    }

    /// Amplitude of the component of `signal` at `frequency`, using a Hann
    /// window so that neighbouring components do not leak in.
    fn amplitude_at(signal: &[f32], frequency: f64) -> f64 {
        let (mut re, mut im, mut weight) = (0.0, 0.0, 0.0);
        let length = signal.len() as f64;
        for (n, sample) in signal.iter().enumerate() {
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * n as f64 / length).cos();
            let phase = std::f64::consts::TAU * frequency * n as f64 / RATE;
            re += f64::from(*sample) * window * phase.cos();
            im += f64::from(*sample) * window * phase.sin();
            weight += window;
        }
        2.0 * (re * re + im * im).sqrt() / weight
    }

    #[test]
    fn residuals_meet_at_their_joints() {
        // The step residual jumps by exactly -1 at the step, which cancels
        // the naive jump, and is smooth everywhere else.
        assert!((blep(-1.0e-6) - 0.5).abs() < 1e-5);
        assert!((blep(0.0) + 0.5).abs() < 1e-6);
        assert!((blep(-1.0) - 1.0 / 24.0).abs() < 1e-6);
        assert!((blep(-1.0 - 1e-6) - 1.0 / 24.0).abs() < 1e-5);
        assert!((blep(1.0) + 1.0 / 24.0).abs() < 1e-6);
        assert!(blep(-2.0).abs() < 1e-7);
        assert!(blep(1.999_99).abs() < 1e-7);
        for step in 0..400 {
            let t = step as f32 / 100.0 - 2.0;
            if t.abs() > 0.02 {
                assert!((blep(t) + blep(-t)).abs() < 1e-5, "odd symmetry at {t}");
            }
        }
        assert!((blamp(0.0) - 7.0 / 30.0).abs() < 1e-6);
        assert!((blamp(1.0) - 1.0 / 120.0).abs() < 1e-6);
        assert!((blamp(0.999_99) - 1.0 / 120.0).abs() < 1e-5);
        assert!(blamp(2.0).abs() < 1e-7);
        assert_eq!(blamp(-0.7), blamp(0.7));
    }

    #[test]
    fn waveforms_have_the_right_overtones() {
        let frequency = 220.0;
        let length = 1 << 16;
        // A saw has every overtone at 1/n; the rounding dulls the highest
        // by a known, small amount.
        let saw = render(Waveform::Saw, frequency, 0.5, length);
        for harmonic in 1..=10 {
            let level = amplitude_at(&saw, frequency * f64::from(harmonic));
            let ideal = 2.0 / (std::f64::consts::PI * f64::from(harmonic));
            assert!(
                (level / ideal - 1.0).abs() < 0.02,
                "saw harmonic {harmonic}: {level}"
            );
        }
        let square = render(Waveform::Square, frequency, 0.5, length);
        for harmonic in 1..=9 {
            let level = amplitude_at(&square, frequency * f64::from(harmonic));
            if harmonic % 2 == 1 {
                let ideal = 4.0 / (std::f64::consts::PI * f64::from(harmonic));
                assert!(
                    (level / ideal - 1.0).abs() < 0.02,
                    "square harmonic {harmonic}"
                );
            } else {
                assert!(level < 2e-3, "square harmonic {harmonic}: {level}");
            }
        }
        let triangle = render(Waveform::Triangle, frequency, 0.5, length);
        for harmonic in [1, 3, 5, 7] {
            let level = amplitude_at(&triangle, frequency * f64::from(harmonic));
            let ideal = 8.0 / (std::f64::consts::PI.powi(2) * f64::from(harmonic * harmonic));
            assert!(
                (level / ideal - 1.0).abs() < 0.02,
                "triangle harmonic {harmonic}"
            );
        }
        assert!(amplitude_at(&triangle, frequency * 2.0) < 1e-3);
        let sine = render(Waveform::Sine, frequency, 0.5, length);
        assert!((amplitude_at(&sine, frequency) - 1.0).abs() < 1e-3);
        assert!(amplitude_at(&sine, frequency * 2.0) < 1e-4);
        assert!(amplitude_at(&sine, frequency * 3.0) < 1e-4);
    }

    #[test]
    fn pulse_width_sets_the_overtones_and_never_adds_an_offset() {
        // A pulse of width w has overtone n at (4 / (pi n)) * sin(pi n w).
        for width in [0.1_f32, 0.25, 0.5, 0.8] {
            let pulse = render(Waveform::Pulse, 200.0, width, 1 << 16);
            let mean: f64 = pulse.iter().map(|s| f64::from(*s)).sum::<f64>() / pulse.len() as f64;
            assert!(mean.abs() < 2e-3, "width {width}: mean {mean}");
            for harmonic in 1..=6 {
                let n = f64::from(harmonic);
                let ideal = (4.0 / (std::f64::consts::PI * n)
                    * (std::f64::consts::PI * n * f64::from(width)).sin())
                .abs();
                let level = amplitude_at(&pulse, 200.0 * n);
                assert!(
                    (level - ideal).abs() < 0.02,
                    "width {width} harmonic {harmonic}"
                );
            }
        }
    }

    /// Level of the loudest component that is not an overtone of
    /// `frequency`, relative to the fundamental, in dB.
    fn worst_alias_db(waveform: Waveform, frequency: f64, below_hz: f64) -> f64 {
        let length = 1 << 15;
        let signal = render(waveform, frequency, 0.3, length);
        let fundamental = amplitude_at(&signal, frequency);
        // Everything above half the sample rate folds back to these.
        let mut worst = 0.0_f64;
        for harmonic in 1..400 {
            let true_frequency = frequency * f64::from(harmonic);
            if true_frequency < RATE / 2.0 {
                continue;
            }
            let folded = (true_frequency % RATE).min(RATE - true_frequency % RATE);
            let near_harmonic = (folded / frequency - (folded / frequency).round()).abs() < 0.02;
            if folded < below_hz && folded > 20.0 && !near_harmonic {
                worst = worst.max(amplitude_at(&signal, folded));
            }
        }
        20.0 * (worst / fundamental).log10()
    }

    #[test]
    fn aliasing_stays_far_below_the_fundamental() {
        // Measured at the plain sample rate. The synth runs these at twice
        // the rate, which pushes the figures down much further.
        for waveform in [Waveform::Saw, Waveform::Pulse, Waveform::Square] {
            let mid = worst_alias_db(waveform, 1_244.5, 12_000.0);
            assert!(mid < -65.0, "{waveform:?} at 1.2 kHz: {mid} dB");
            let high = worst_alias_db(waveform, 3_520.0, 12_000.0);
            assert!(high < -65.0, "{waveform:?} at 3.5 kHz: {high} dB");
        }
        let triangle = worst_alias_db(Waveform::Triangle, 3_520.0, 16_000.0);
        assert!(triangle < -80.0, "triangle: {triangle} dB");
    }

    #[test]
    fn a_naive_saw_would_fail_the_aliasing_bound() {
        // Guards the measurement itself: without the correction the same
        // check reads tens of decibels worse.
        let length = 1 << 15;
        let frequency = 3_520.0;
        let mut phase = 0.0_f64;
        let naive: Vec<f32> = (0..length)
            .map(|_| {
                let value = (2.0 * phase - 1.0) as f32;
                phase = (phase + frequency / RATE).fract();
                value
            })
            .collect();
        let fundamental = amplitude_at(&naive, frequency);
        // The 7th overtone, 24.64 kHz, folds to 23.36 kHz; the 9th, 31.68,
        // to 16.32; the 11th, 38.72, to 9.28.
        let alias = amplitude_at(&naive, 48_000.0 - 11.0 * frequency);
        assert!(20.0 * (alias / fundamental).log10() > -30.0);
    }

    #[test]
    fn output_stays_within_a_small_margin_of_full_scale() {
        for waveform in [
            Waveform::Sine,
            Waveform::Triangle,
            Waveform::Saw,
            Waveform::Square,
            Waveform::Pulse,
        ] {
            for frequency in [20.0, 440.0, 5_000.0, 11_000.0] {
                for width in [0.05, 0.5, 0.95] {
                    let peak = render(waveform, frequency, width, 20_000)
                        .iter()
                        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
                    assert!(peak < 2.0, "{waveform:?} {frequency} {width}: {peak}");
                }
            }
        }
    }

    #[test]
    fn noise_waveforms_have_no_tonal_sample() {
        assert!(Waveform::WhiteNoise.is_noise() && Waveform::PinkNoise.is_noise());
        assert!(!Waveform::Saw.is_noise());
        assert_eq!(tonal_sample(Waveform::WhiteNoise, 0.3, 0.01, 0.5), 0.0);
    }

    #[test]
    fn phase_wraps_and_frequency_is_exact() {
        let mut oscillator = Oscillator::new(1.25);
        assert!((oscillator.phase() - 0.25).abs() < 1e-6);
        let mut wraps = 0;
        let mut previous = oscillator.phase();
        for _ in 0..48_000 {
            oscillator.tick(Waveform::Sine, 440.0 / 48_000.0, 0.5);
            if oscillator.phase() < previous {
                wraps += 1;
            }
            previous = oscillator.phase();
        }
        assert_eq!(wraps, 440);
    }
}
