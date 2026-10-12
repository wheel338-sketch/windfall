//! Adaptive source/filter formants. Own LPC estimator, no neural/content model.
//! Prediction coefficients and warped filter memories are bounded fixed arrays.
const ORDER: usize = 128;
const WINDOW: usize = 4096;
pub(super) struct Formants {
    rate_scale: f64,
    order: usize,
    history: [[f32; WINDOW * 2]; 2],
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
            rate_scale: 1.0,
            order: 8,
            history: [[0.0; WINDOW * 2]; 2],
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
    pub fn prepare(&mut self, sample_rate: f32) {
        self.rate_scale = (48000.0 / f64::from(sample_rate)).min(1.0);
        self.reset();
    }
    pub fn reset(&mut self) {
        let rate_scale = self.rate_scale;
        *self = Self::default();
        self.rate_scale = rate_scale;
    }
    fn estimate(&mut self, channel: usize) {
        let order = self.order;
        let window = (512.0 / self.rate_scale).round().min(WINDOW as f64) as usize;
        let history = &self.history[channel][self.cursor..self.cursor + window];
        let mut correlation = [0.0_f64; ORDER + 1];
        for (lag, r) in correlation.iter_mut().enumerate().take(order + 1) {
            let (left, remainder) = history[lag..].as_chunks::<4>();
            let (right, rest) = history[..window - lag].as_chunks::<4>();
            let mut lanes = [0.0_f64; 4];
            for (a, b) in left.iter().zip(right) {
                for i in 0..4 {
                    lanes[i] += f64::from(a[i]) * f64::from(b[i]);
                }
            }
            *r = lanes.iter().sum();
            for (a, b) in remainder.iter().zip(rest) {
                *r += f64::from(*a) * f64::from(*b);
            }
        }
        self.target[channel].fill(0.0);
        if correlation[0] < 1e-6 {
            return;
        }
        // Diagonal loading and bandwidth expansion keep the all-pole envelope
        // conservative; reflection coefficients are bounded before recursion.
        let mut energy = correlation[0] * (1.0 + 0.01 * self.rate_scale.powi(2));
        let mut coefficients = [0.0; ORDER + 1];
        coefficients[0] = 1.0;
        for i in 1..=order {
            let residual = correlation[i]
                + (1..i)
                    .map(|j| coefficients[j] * correlation[i - j])
                    .sum::<f64>();
            let bound = 0.95_f64.powf(self.rate_scale);
            let reflection = (-residual / energy.max(1e-12)).clamp(-bound, bound);
            let old = coefficients;
            for j in 1..i {
                coefficients[j] = old[j] + reflection * old[i - j];
            }
            coefficients[i] = reflection;
            energy *= 1.0 - reflection * reflection;
        }
        for (i, a) in coefficients.iter().enumerate().take(order + 1).skip(1) {
            self.target[channel][i] = *a * 0.985_f64.powf(i as f64 * self.rate_scale);
        }
    }
    /// Whitening before resampling leaves a pitch-independent vocal envelope.
    pub fn excitation(&mut self, input: [f32; 2], voice: bool) -> [f32; 2] {
        self.order = ((if voice { 16.0 } else { 8.0 }) / self.rate_scale)
            .round()
            .min(ORDER as f64) as usize;
        let window = (512.0 / self.rate_scale).round().min(WINDOW as f64) as usize;
        for (ch, value) in input.iter().enumerate() {
            self.history[ch][self.cursor] = *value;
            self.history[ch][self.cursor + window] = *value;
        }
        self.cursor = (self.cursor + 1) % window;
        self.hop += 1;
        if self.hop >= (128.0 / self.rate_scale).round() as usize {
            self.hop = 0;
            self.estimate(0);
            // Autocorrelation is identical for equal or polarity-inverted
            // histories. Share that exact envelope, retaining independent
            // whitening/filter memories and full estimation for other stereo.
            let left = &self.history[0][self.cursor..self.cursor + window];
            let right = &self.history[1][self.cursor..self.cursor + window];
            if left.iter().zip(right).all(|(a, b)| a == b)
                || left.iter().zip(right).all(|(a, b)| *a == -*b)
            {
                self.target[1] = self.target[0];
            } else {
                self.estimate(1);
            }
        }
        std::array::from_fn(|ch| {
            let mut residual = f64::from(input[ch]);
            for i in 0..self.order {
                self.coefficients[ch][i + 1] +=
                    (self.target[ch][i + 1] - self.coefficients[ch][i + 1]) * 0.01;
                residual += self.coefficients[ch][i + 1] * self.input[ch][i];
            }
            self.input[ch].copy_within(0..self.order - 1, 1);
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
            for i in 0..self.order {
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
            for i in 0..self.order {
                let next = -warp * value + self.memory[ch][i];
                self.memory[ch][i] = value + warp * next;
                value = next;
            }
            super::output(result as f32)
        })
    }
}
