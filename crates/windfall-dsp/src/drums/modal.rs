//! Truncated circular-membrane impulse response and a damped cavity mode.
//! Eigenfrequency ratios are normalized Bessel zeros (j_01 = 2.4048256).
//! Positive modal losses and bounded excitation make this passive at every
//! setting: there is no unstable feedback path or callback-owned heap data.
use crate::blocks::oscillator::{Oscillator, Waveform};

const RATIOS: [f32; 8] = [
    1.0, 1.59334, 2.13555, 2.29542, 2.65307, 2.9173, 3.15546, 3.50015,
];

#[derive(Clone, Copy)]
pub(super) struct ModalDrum {
    modes: [Oscillator; 9],
    energy: [f32; 9],
}
impl Default for ModalDrum {
    fn default() -> Self {
        Self {
            modes: [Oscillator::default(); 9],
            energy: [1.0; 9],
        }
    }
}
impl ModalDrum {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn tick(
        &mut self,
        pitch: f32,
        decay: f32,
        tone: f32,
        strike: f32,
        damping: f32,
        body: f32,
        rate: f32,
    ) -> f32 {
        let mut sum = 0.0;
        for (i, ratio) in RATIOS.iter().copied().enumerate() {
            let n = (i + 1) as f32;
            // Radial mode-shape approximation: edge strikes cancel different
            // modes and excite upper modes independently of decay/tuning.
            let shape = (std::f32::consts::PI * strike * n).cos() * (1.0 - strike * strike);
            let weight = if i == 0 { 1.0 } else { (0.12 + 0.6 * tone) / n };
            let frequency = pitch * ratio;
            let fade = ((0.24 * rate - frequency) / (0.02 * rate)).clamp(0.0, 1.0);
            sum += self.modes[i].tick(Waveform::Sine, (frequency / rate).min(0.24), 0.5)
                * self.energy[i]
                * shape
                * weight
                * fade;
            let loss = (1.0 + damping * (ratio * ratio - 1.0)) / (decay * 0.001 * rate);
            self.energy[i] = crate::blocks::math::flush(self.energy[i] * (-3.0 * loss).exp());
        }
        sum += self.modes[8].tick(Waveform::Sine, (pitch * 0.63 / rate).min(0.24), 0.5)
            * self.energy[8]
            * body
            * 0.3;
        self.energy[8] =
            crate::blocks::math::flush(self.energy[8] * (-5.0 / (decay * 0.001 * rate)).exp());
        sum * 0.65
    }
}
