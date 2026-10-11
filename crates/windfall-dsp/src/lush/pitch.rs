//! Feed-forward shimmer. Two complementary grains read a rate-scaled history;
//! convex interpolation/windows cannot amplify the peak of the wet input.
use crate::blocks::delay_line::DelayLine;

pub(super) struct PitchVoice {
    lines: [DelayLine; 2],
    phase: f64,
    span: usize,
}

impl Default for PitchVoice {
    fn default() -> Self {
        Self {
            lines: std::array::from_fn(|_| DelayLine::default()),
            phase: 0.5,
            span: 32,
        }
    }
}

impl PitchVoice {
    pub fn prepare(&mut self, rate: f32) {
        self.span = (f64::from(rate) * 0.04).round().max(32.0) as usize;
        self.lines = std::array::from_fn(|_| DelayLine::new(self.span + 4));
        self.clear();
    }
    pub fn clear(&mut self) {
        for line in &mut self.lines {
            line.clear();
        }
        self.phase = 0.5;
    }
    pub fn history(&self) -> usize {
        self.span + 4
    }
    pub fn tick(&mut self, input: [f32; 2], ratio: f32) -> [f32; 2] {
        let p = self.phase as f32;
        let q = (p + 0.5).fract();
        let weight = 0.5 - 0.5 * (std::f32::consts::TAU * p).cos();
        let a = 2.0 + p * self.span as f32;
        let b = 2.0 + q * self.span as f32;
        let mut output = [0.0; 2];
        for ch in 0..2 {
            output[ch] = self.lines[ch].tap_linear(a) * weight
                + self.lines[ch].tap_linear(b) * (1.0 - weight);
            self.lines[ch].push(input[ch]);
        }
        self.phase = (self.phase + (1.0 - f64::from(ratio)) / self.span as f64).rem_euclid(1.0);
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_octave_voice_moves_the_measured_spectral_peak_and_preserves_stereo() {
        let rate = 16_000.0;
        for (ratio, target) in [(0.5, 250.0), (1.0, 500.0), (2.0, 1000.0)] {
            let mut pitch = PitchVoice::default();
            pitch.prepare(rate);
            let mut output = Vec::new();
            for n in 0..32_000 {
                let input = 0.2 * (std::f32::consts::TAU * 500.0 * n as f32 / rate).sin();
                let shifted = pitch.tick([input, -input], ratio);
                assert_eq!(shifted[0], -shifted[1]);
                assert!(shifted[0].abs() <= 0.20001);
                if n >= 8000 {
                    output.push(shifted[0]);
                }
            }
            // Direct sinusoidal projection, no production FFT or pitch detector.
            let power = |hz: f64| {
                let mut re = 0.0;
                let mut im = 0.0;
                for (n, x) in output.iter().enumerate() {
                    let angle = std::f64::consts::TAU * hz * n as f64 / f64::from(rate);
                    re += f64::from(*x) * angle.cos();
                    im += f64::from(*x) * angle.sin();
                }
                re * re + im * im
            };
            assert!(power(target) > 1000.0);
            if ratio != 1.0 {
                assert!(power(target) > 20.0 * power(500.0));
            }
        }
    }
}
