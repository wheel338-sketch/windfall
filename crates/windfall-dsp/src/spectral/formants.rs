//! Adaptive source/filter formants. Own LPC estimator, no neural/content model.
//! Prediction coefficients and warped filter memories are bounded fixed arrays.
const ORDER: usize = 16;
const WINDOW: usize = 512;
pub(super) struct Formants {
    history: [[f32; WINDOW]; 2],
    cursor: usize,
    hop: usize,
    coefficients: [[f64; ORDER + 1]; 2],
    target: [[f64; ORDER + 1]; 2],
    input: [[f64; ORDER]; 2],
    memory: [[f64; ORDER]; 2],
}
impl Default for Formants {
    fn default() -> Self {
        Self {
            history: [[0.0; WINDOW]; 2],
            cursor: 0,
            hop: 0,
            coefficients: [[0.0; ORDER + 1]; 2],
            target: [[0.0; ORDER + 1]; 2],
            input: [[0.0; ORDER]; 2],
            memory: [[0.0; ORDER]; 2],
        }
    }
}
impl Formants {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    fn estimate(&mut self, channel: usize, voice: bool) {
        let order = if voice { ORDER } else { 8 };
        let mut correlation = [0.0_f64; ORDER + 1];
        for (lag, r) in correlation.iter_mut().enumerate().take(order + 1) {
            for i in lag..WINDOW {
                let a = f64::from(self.history[channel][(self.cursor + i) % WINDOW]);
                let b = f64::from(self.history[channel][(self.cursor + i - lag) % WINDOW]);
                *r += a * b;
            }
        }
        self.target[channel].fill(0.0);
        if correlation[0] < 1e-6 {
            return;
        }
        // Diagonal loading and bandwidth expansion keep the all-pole envelope
        // conservative; reflection coefficients are bounded before recursion.
        let mut energy = correlation[0] * 1.01;
        let mut coefficients = [0.0; ORDER + 1];
        coefficients[0] = 1.0;
        for i in 1..=order {
            let residual = correlation[i]
                + (1..i)
                    .map(|j| coefficients[j] * correlation[i - j])
                    .sum::<f64>();
            let reflection = (-residual / energy.max(1e-12)).clamp(-0.95, 0.95);
            let old = coefficients;
            for j in 1..i {
                coefficients[j] = old[j] + reflection * old[i - j];
            }
            coefficients[i] = reflection;
            energy *= 1.0 - reflection * reflection;
        }
        for (i, a) in coefficients.iter().enumerate().take(order + 1).skip(1) {
            self.target[channel][i] = *a * 0.985_f64.powi(i as i32);
        }
    }
    /// Whitening before resampling leaves a pitch-independent vocal envelope.
    pub fn excitation(&mut self, input: [f32; 2], voice: bool) -> [f32; 2] {
        for (ch, value) in input.iter().enumerate() {
            self.history[ch][self.cursor] = *value;
        }
        self.cursor = (self.cursor + 1) % WINDOW;
        self.hop += 1;
        if self.hop == 128 {
            self.hop = 0;
            for ch in 0..2 {
                self.estimate(ch, voice);
            }
        }
        std::array::from_fn(|ch| {
            let mut residual = f64::from(input[ch]);
            for i in 0..ORDER {
                self.coefficients[ch][i + 1] +=
                    (self.target[ch][i + 1] - self.coefficients[ch][i + 1]) * 0.01;
                residual += self.coefficients[ch][i + 1] * self.input[ch][i];
            }
            self.input[ch].copy_within(0..ORDER - 1, 1);
            self.input[ch][0] = f64::from(input[ch]);
            super::output(residual as f32)
        })
    }
    /// First-order allpass frequency warping controls envelope/formants without
    /// retiming the excitation. Solve the current-sample feedback algebraically.
    pub fn synthesize(&mut self, excitation: [f32; 2], semitones: f32) -> [f32; 2] {
        let ratio = f64::from((semitones / 12.0).exp2());
        let warp = ((1.0 - ratio) / (1.0 + ratio)).clamp(-0.6, 0.6);
        std::array::from_fn(|ch| {
            let mut factor = 1.0;
            let mut residual = 0.0;
            let mut denominator = 1.0;
            let mut sum = 0.0;
            for i in 0..ORDER {
                factor *= -warp;
                residual = -warp * residual + self.memory[ch][i];
                denominator += self.coefficients[ch][i + 1] * factor;
                sum += self.coefficients[ch][i + 1] * residual;
            }
            let mut value = if denominator.abs() > 0.05 {
                (f64::from(excitation[ch]) - sum) / denominator
            } else {
                0.0
            };
            if !value.is_finite() || value.abs() > 1000.0 {
                self.memory[ch].fill(0.0);
                value = 0.0;
            }
            let result = value;
            for i in 0..ORDER {
                let next = -warp * value + self.memory[ch][i];
                self.memory[ch][i] = value + warp * next;
                value = next;
            }
            super::output(result as f32)
        })
    }
}
