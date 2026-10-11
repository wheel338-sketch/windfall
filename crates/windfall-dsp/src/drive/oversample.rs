//! Runs a nonlinearity at two, four or eight times the sample rate, with
//! the same delay at every setting.
//!
//! The design is the one `crate::distortion` measures: a Blackman-windowed
//! sinc, used once as a polyphase interpolator ahead of the nonlinearity and
//! once as the filter ahead of decimation. Its length grows with the factor
//! (`32 * factor + 1` taps), which makes the delay 32 output samples for
//! every factor. "Off" delays by the same 32 samples and shapes at the host
//! rate, so a host never has to change its delay compensation when the
//! quality is changed or automated.
//!
//! The kernels do not depend on the sample rate. They are built once per
//! process, away from the audio thread, and shared by every processor.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Samples by which every oversampled drive stage delays its signal.
pub const DRIVE_LATENCY_SAMPLES: usize = 32;

/// Base-rate samples each polyphase branch reaches back over.
const INPUT_TAPS: usize = DRIVE_LATENCY_SAMPLES + 1;
const MAX_FACTOR: usize = 8;
const MAX_TAPS: usize = DRIVE_LATENCY_SAMPLES * MAX_FACTOR + 1;
/// Output samples over which a change of quality fades in.
const SWITCH_FADE: u32 = 96;

/// How many times the host rate a nonlinear stage runs at.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Oversampling {
    /// The host rate. Cheapest, and it aliases.
    Off,
    X2,
    #[default]
    X4,
    X8,
}

impl Oversampling {
    pub const ALL: [Self; 4] = [Self::Off, Self::X2, Self::X4, Self::X8];

    pub fn factor(self) -> usize {
        match self {
            Self::Off => 1,
            Self::X2 => 2,
            Self::X4 => 4,
            Self::X8 => 8,
        }
    }
}

/// The filters of one factor.
struct Kernel {
    /// `DRIVE_LATENCY_SAMPLES * factor + 1` taps at the high rate.
    down: Box<[f64]>,
    /// One branch per output phase. Each adds up to one.
    up: Box<[[f64; INPUT_TAPS]]>,
}

impl Kernel {
    fn new(factor: usize) -> Self {
        let taps = DRIVE_LATENCY_SAMPLES * factor + 1;
        // 90% of the host's Nyquist frequency, as a share of the high rate.
        let cutoff = 0.45 / factor as f64;
        let mut down = vec![0.0_f64; taps];
        for (n, coefficient) in down.iter_mut().enumerate() {
            let x = n as f64 - (taps - 1) as f64 * 0.5;
            let sinc = if x == 0.0 {
                2.0 * cutoff
            } else {
                (std::f64::consts::TAU * cutoff * x).sin() / (std::f64::consts::PI * x)
            };
            let phase = std::f64::consts::TAU * n as f64 / (taps - 1) as f64;
            *coefficient = sinc * (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos());
        }
        let sum: f64 = down.iter().sum();
        for coefficient in &mut down {
            *coefficient /= sum;
        }
        let mut up = vec![[0.0_f64; INPUT_TAPS]; factor];
        for (n, coefficient) in down.iter().enumerate() {
            up[n % factor][n / factor] = *coefficient;
        }
        for branch in &mut up {
            let sum: f64 = branch.iter().sum();
            for coefficient in branch.iter_mut() {
                *coefficient /= sum;
            }
        }
        Self {
            down: down.into_boxed_slice(),
            up: up.into_boxed_slice(),
        }
    }
}

/// The filters of every factor. Shared and never changed once built.
pub(crate) struct Kernels {
    by_factor: [Kernel; 3],
}

impl Kernels {
    /// The shared kernels, built on first use. Allocates the first time, so
    /// processors call it from `prepare`.
    pub(crate) fn shared() -> &'static Kernels {
        static KERNELS: OnceLock<Kernels> = OnceLock::new();
        KERNELS.get_or_init(|| Kernels {
            by_factor: [Kernel::new(2), Kernel::new(4), Kernel::new(8)],
        })
    }

    fn of(&self, quality: Oversampling) -> Option<&Kernel> {
        match quality {
            Oversampling::Off => None,
            Oversampling::X2 => Some(&self.by_factor[0]),
            Oversampling::X4 => Some(&self.by_factor[1]),
            Oversampling::X8 => Some(&self.by_factor[2]),
        }
    }
}

/// One channel of an oversampled nonlinear stage.
///
/// Holds fixed arrays only, so nothing here allocates. A processor keeps one
/// per channel and hands every call the same shared [`Kernels`].
pub(crate) struct Oversampler {
    input: [f64; INPUT_TAPS],
    high: [f64; MAX_TAPS],
    input_at: usize,
    high_at: usize,
    quality: Oversampling,
    last: f32,
    from: f32,
    fade: u32,
}

impl Default for Oversampler {
    fn default() -> Self {
        Self {
            input: [0.0; INPUT_TAPS],
            high: [0.0; MAX_TAPS],
            input_at: 0,
            high_at: 0,
            quality: Oversampling::default(),
            last: 0.0,
            from: 0.0,
            fade: 0,
        }
    }
}

impl Oversampler {
    pub(crate) fn reset(&mut self) {
        self.input.fill(0.0);
        self.high.fill(0.0);
        self.input_at = 0;
        self.high_at = 0;
        self.last = 0.0;
        self.from = 0.0;
        self.fade = 0;
    }

    /// Picks the factor. A change clears the high-rate memory, which holds
    /// samples at the old rate, and fades the output in from where it was.
    /// `snap` is for settings given before the first sample: nothing has
    /// been heard yet, so there is nothing to fade from.
    pub(crate) fn set_quality(&mut self, quality: Oversampling, snap: bool) {
        if quality == self.quality {
            return;
        }
        self.quality = quality;
        self.high.fill(0.0);
        self.high_at = 0;
        if !snap {
            self.from = self.last;
            self.fade = SWITCH_FADE;
        }
    }

    /// Passes one sample through `shape`, `DRIVE_LATENCY_SAMPLES` late.
    /// `shape` runs once per high-rate sample.
    #[inline]
    pub(crate) fn tick(
        &mut self,
        kernels: Option<&Kernels>,
        input: f32,
        mut shape: impl FnMut(f64) -> f64,
    ) -> f32 {
        self.input[self.input_at] = f64::from(input);
        let quality = self.quality;
        let kernel = kernels.and_then(|kernels| kernels.of(quality));
        let shaped = match kernel {
            None => {
                // The oldest sample in the ring is the one 32 samples back.
                let delayed = self.input[(self.input_at + 1) % INPUT_TAPS];
                shape(delayed)
            }
            Some(kernel) => {
                let taps = kernel.down.len();
                let mut output = 0.0;
                for (phase, branch) in kernel.up.iter().enumerate() {
                    let mut interpolated = 0.0;
                    for (k, coefficient) in branch.iter().enumerate() {
                        interpolated += coefficient
                            * self.input[(self.input_at + INPUT_TAPS - k) % INPUT_TAPS];
                    }
                    self.high[self.high_at] = shape(interpolated);
                    if phase == 0 {
                        for (k, coefficient) in kernel.down.iter().enumerate() {
                            output += coefficient * self.high[(self.high_at + taps - k) % taps];
                        }
                    }
                    self.high_at = (self.high_at + 1) % taps;
                }
                output
            }
        };
        self.input_at = (self.input_at + 1) % INPUT_TAPS;
        let mut output = shaped as f32;
        if !output.is_finite() {
            output = 0.0;
        }
        if self.fade > 0 {
            self.fade -= 1;
            let arrived = 1.0 - self.fade as f32 / SWITCH_FADE as f32;
            output = self.from + (output - self.from) * arrived;
        }
        self.last = crate::blocks::math::flush(output);
        self.last
    }
}

/// A plain delay of `DRIVE_LATENCY_SAMPLES`, for a linear stage that sits
/// in a chain of oversampled ones and has to stay in step with them.
#[derive(Clone, Copy)]
pub(crate) struct LatencyDelay {
    ring: [f32; DRIVE_LATENCY_SAMPLES],
    at: usize,
}

impl Default for LatencyDelay {
    fn default() -> Self {
        Self {
            ring: [0.0; DRIVE_LATENCY_SAMPLES],
            at: 0,
        }
    }
}

impl LatencyDelay {
    pub(crate) fn reset(&mut self) {
        self.ring.fill(0.0);
        self.at = 0;
    }

    #[inline]
    pub(crate) fn tick(&mut self, input: f32) -> f32 {
        let delayed = self.ring[self.at];
        self.ring[self.at] = input;
        self.at = (self.at + 1) % DRIVE_LATENCY_SAMPLES;
        delayed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(quality: Oversampling, input: &[f32], shape: impl Fn(f64) -> f64) -> Vec<f32> {
        let kernels = Kernels::shared();
        let mut stage = Oversampler::default();
        stage.set_quality(quality, true);
        input
            .iter()
            .map(|sample| stage.tick(Some(kernels), *sample, &shape))
            .collect()
    }

    fn sine(hz: f64, level: f64, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| (level * (std::f64::consts::TAU * hz * n as f64 / 48_000.0).sin()) as f32)
            .collect()
    }

    /// Amplitude of the component at `hz`, read over the settled part.
    fn level(signal: &[f32], hz: f64) -> f64 {
        let skip = 1_024;
        let (mut re, mut im) = (0.0, 0.0);
        for (n, sample) in signal.iter().enumerate().skip(skip) {
            let phase = std::f64::consts::TAU * hz * n as f64 / 48_000.0;
            re += f64::from(*sample) * phase.cos();
            im += f64::from(*sample) * phase.sin();
        }
        (re * re + im * im).sqrt() * 2.0 / (signal.len() - skip) as f64
    }

    #[test]
    fn every_quality_delays_by_exactly_the_reported_latency() {
        let mut impulse = vec![0.0_f32; 256];
        impulse[10] = 1.0;
        for quality in Oversampling::ALL {
            let output = run(quality, &impulse, |x| x);
            let peak = output
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
                .map(|(index, _)| index)
                .unwrap();
            assert_eq!(peak, 10 + DRIVE_LATENCY_SAMPLES, "{quality:?}");
        }
        // Off is the input itself, late.
        let ramp: Vec<f32> = (0..200).map(|n| n as f32 * 0.001).collect();
        let output = run(Oversampling::Off, &ramp, |x| x);
        assert!(output[..DRIVE_LATENCY_SAMPLES].iter().all(|x| *x == 0.0));
        assert_eq!(&output[DRIVE_LATENCY_SAMPLES..], &ramp[..200 - DRIVE_LATENCY_SAMPLES]);
    }

    #[test]
    fn an_identity_shape_passes_the_audible_band_at_unity() {
        for quality in [Oversampling::X2, Oversampling::X4, Oversampling::X8] {
            for hz in [100.0, 1_000.0, 10_000.0, 16_000.0] {
                let output = run(quality, &sine(hz, 0.5, 8_192), |x| x);
                let gain = level(&output, hz) / 0.5;
                assert!((gain - 1.0).abs() < 0.01, "{quality:?} at {hz} Hz: {gain}");
            }
            let output = run(quality, &vec![0.25; 512], |x| x);
            assert!((output[400] - 0.25).abs() < 1.0e-5, "{quality:?} at DC");
        }
    }

    #[test]
    fn oversampling_removes_the_aliases_a_soft_clipper_makes_at_the_host_rate() {
        // 5 kHz driven hard through a smooth odd curve. Its ninth harmonic,
        // 45 kHz, folds to 3 kHz at 48 kHz; its seventh, 35 kHz, to 13 kHz.
        // Neither frequency is a harmonic of 5 kHz, so anything measured
        // there is aliasing.
        let input = sine(5_000.0, 1.0, 16_384);
        let shape = |x: f64| (4.0 * x).tanh();
        let naive = run(Oversampling::Off, &input, shape);
        let naive_alias = level(&naive, 3_000.0).max(level(&naive, 13_000.0));
        assert!(naive_alias > 1.0e-3, "the host rate aliases: {naive_alias}");
        for quality in [Oversampling::X2, Oversampling::X4, Oversampling::X8] {
            let output = run(quality, &input, shape);
            let alias = level(&output, 3_000.0).max(level(&output, 13_000.0));
            // 40 dB is far inside what the filters' stopband gives.
            assert!(
                alias < naive_alias * 0.01,
                "{quality:?}: {alias} against {naive_alias} at the host rate"
            );
            // The fundamental is untouched by the filters.
            let reference = level(&naive, 5_000.0);
            assert!((level(&output, 5_000.0) / reference - 1.0).abs() < 0.02);
        }
    }

    #[test]
    fn a_bounded_shape_overshoots_its_ceiling_by_a_bounded_amount() {
        // The decimation filter rings on a clipped square, so the output
        // can exceed the shape's ceiling. It stays within the filter's
        // known overshoot.
        let input = sine(997.0, 8.0, 8_192);
        for quality in Oversampling::ALL {
            let output = run(quality, &input, |x| x.clamp(-0.5, 0.5));
            let peak = output.iter().fold(0.0_f32, |peak, x| peak.max(x.abs()));
            assert!(peak <= 0.5 * 1.25, "{quality:?} peaks at {peak}");
            assert!(peak >= 0.45, "{quality:?} peaks at {peak}");
        }
    }

    #[test]
    fn changing_quality_mid_stream_stays_finite_and_fades() {
        let kernels = Kernels::shared();
        let mut stage = Oversampler::default();
        stage.set_quality(Oversampling::X4, true);
        let input = sine(440.0, 0.5, 4_096);
        let mut output = Vec::new();
        for (n, sample) in input.iter().enumerate() {
            if n == 2_000 {
                stage.set_quality(Oversampling::Off, false);
            }
            if n == 3_000 {
                stage.set_quality(Oversampling::X8, false);
            }
            output.push(stage.tick(Some(kernels), *sample, |x| x));
        }
        assert!(output.iter().all(|x| x.is_finite()));
        // No step larger than the signal's own slope plus the fade's.
        let steepest = output[1_000..]
            .windows(2)
            .fold(0.0_f32, |steepest, pair| steepest.max((pair[1] - pair[0]).abs()));
        assert!(steepest < 0.2, "steepest step {steepest}");
    }

    #[test]
    fn the_plain_delay_matches_the_oversampler() {
        let mut delay = LatencyDelay::default();
        let output: Vec<f32> = (0..100).map(|n| delay.tick(n as f32)).collect();
        assert!(output[..DRIVE_LATENCY_SAMPLES].iter().all(|x| *x == 0.0));
        assert_eq!(output[DRIVE_LATENCY_SAMPLES], 0.0);
        assert_eq!(output[DRIVE_LATENCY_SAMPLES + 5], 5.0);
        delay.reset();
        assert_eq!(delay.tick(1.0), 0.0);
    }

    #[test]
    fn unprepared_processing_is_the_host_rate_path() {
        let mut stage = Oversampler::default();
        let output: Vec<f32> = (0..64).map(|n| stage.tick(None, n as f32, |x| x)).collect();
        assert_eq!(output[DRIVE_LATENCY_SAMPLES + 3], 3.0);
    }
}
