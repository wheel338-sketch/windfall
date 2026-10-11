//! Original dual-envelope onset detector. No FFT, allocation or block clock.
#[derive(Default)]
pub(super) struct Transient {
    fast: f64,
    slow: f64,
    holdoff: usize,
    attack: f64,
    release: f64,
}
impl Transient {
    pub fn prepare(&mut self, rate: f32) {
        self.attack = 1.0 - (-1.0 / (0.00025 * rate as f64)).exp();
        self.release = 1.0 - (-1.0 / (0.025 * rate as f64)).exp();
        self.reset();
    }
    pub fn reset(&mut self) {
        self.fast = 0.0;
        self.slow = 0.0;
        self.holdoff = 0;
    }
    pub fn tick(&mut self, level: f32, threshold: f32, contrast: f32, holdoff: usize) -> bool {
        let level = super::buffer::finite(level).abs() as f64;
        self.fast += (if level > self.fast {
            self.attack
        } else {
            self.release
        }) * (level - self.fast);
        let detected = self.holdoff == 0
            && self.fast >= threshold as f64
            && self.fast > self.slow * contrast as f64;
        self.slow += self.release * (level - self.slow);
        self.fast = crate::blocks::math::flush64(self.fast);
        self.slow = crate::blocks::math::flush64(self.slow);
        self.holdoff = if detected {
            holdoff.max(1)
        } else {
            self.holdoff.saturating_sub(1)
        };
        detected
    }
}
