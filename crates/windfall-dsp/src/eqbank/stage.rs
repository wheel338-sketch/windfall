use crate::blocks::biquad::{Biquad, BiquadCoeffs, CoeffRamp};
use crate::blocks::math::{clean, flush, ms_to_samples};

pub(super) fn rate(value: f32) -> f32 {
    // Keep the cookbook's 1 Hz lower limit below its Nyquist guard.
    clean(value, 8.0, 384_000.0, 48_000.0)
}

pub(super) fn transition_samples(sample_rate: f32, fresh: bool) -> u32 {
    if fresh {
        0
    } else {
        ms_to_samples(5.0, sample_rate).max(1)
    }
}

pub(super) fn input(value: f32) -> f64 {
    if value.is_finite() {
        f64::from(value)
    } else {
        0.0
    }
}

pub(super) fn output(value: f64) -> f32 {
    if value.is_finite() {
        flush(value.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32)
    } else {
        0.0
    }
}

pub(super) fn peak(frequency: f32, gain: f32, q: f32, sample_rate: f32) -> BiquadCoeffs {
    if gain == 0.0 {
        BiquadCoeffs::IDENTITY
    } else {
        BiquadCoeffs::peak(frequency, q, gain, sample_rate)
    }
}

/// A shared stereo section, with sample-counted coefficient and denormal updates.
#[derive(Clone, Copy)]
pub(super) struct Stage {
    coeffs: CoeffRamp,
    target: BiquadCoeffs,
    filters: [Biquad; 2],
    clock: u8,
}

impl Default for Stage {
    fn default() -> Self {
        Self {
            coeffs: CoeffRamp::new(BiquadCoeffs::IDENTITY),
            target: BiquadCoeffs::IDENTITY,
            filters: [Biquad::default(); 2],
            clock: 0,
        }
    }
}

impl Stage {
    pub(super) fn set(&mut self, coeffs: BiquadCoeffs, samples: u32) {
        if samples == 0 || coeffs != self.target {
            self.coeffs.set_target(coeffs, samples);
            self.target = coeffs;
        }
    }

    pub(super) fn reset(&mut self) {
        self.filters = [Biquad::default(); 2];
        self.coeffs.snap(self.target);
        self.clock = 0;
    }

    pub(super) fn tick(&mut self, input: [f64; 2]) -> [f64; 2] {
        let coeffs = *self.coeffs.tick();
        if self.coeffs.is_settled() && coeffs == BiquadCoeffs::IDENTITY {
            self.filters = [Biquad::default(); 2];
            return input;
        }
        let mut result = [0.0; 2];
        for channel in 0..2 {
            result[channel] = self.filters[channel].tick(&coeffs, input[channel]);
            if !result[channel].is_finite() {
                self.filters[channel].reset();
                result[channel] = 0.0;
            }
        }
        self.clock = (self.clock + 1) % 64;
        if self.clock == 0 {
            for filter in &mut self.filters {
                filter.flush();
            }
        }
        result
    }

    /// Conservative decay estimate, including both ends of a live transition.
    pub(super) fn tail_samples(&self) -> usize {
        coefficient_tail(self.coeffs.current()).max(coefficient_tail(&self.target))
    }
}

pub(super) fn coefficient_tail(c: &BiquadCoeffs) -> usize {
    let d = c.a1 * c.a1 - 4.0 * c.a2;
    let radius = if d < 0.0 {
        c.a2.abs().sqrt()
    } else {
        (c.a1.abs() + d.sqrt()) * 0.5
    };
    if radius > 0.0 && radius < 1.0 {
        ((1.0e-6_f64).ln() / radius.ln()).ceil() as usize
    } else {
        0
    }
}
