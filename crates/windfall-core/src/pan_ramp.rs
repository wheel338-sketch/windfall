//! Allocation-free, frame-addressed interpolation between project pan laws.
use crate::PanLaw;
const LAWS: [PanLaw; 4] = [
    PanLaw::Balance,
    PanLaw::ConstantPower,
    PanLaw::Compromise,
    PanLaw::Linear,
];
fn weights(law: PanLaw) -> [f32; 4] {
    let mut value = [0.0; 4];
    value[LAWS.iter().position(|v| *v == law).unwrap()] = 1.0;
    value
}
#[derive(Debug, Clone, Copy)]
pub struct PanLawRamp {
    from: [f32; 4],
    target: PanLaw,
    start: u64,
    end: u64,
}
impl PanLawRamp {
    pub fn at_rest(law: PanLaw) -> Self {
        Self {
            from: weights(law),
            target: law,
            start: 0,
            end: 0,
        }
    }
    pub fn settled(&self, frame: u64) -> bool {
        frame >= self.end
    }
    fn weights_at(&self, frame: u64) -> [f32; 4] {
        let to = weights(self.target);
        if self.settled(frame) {
            return to;
        }
        let part = frame.saturating_sub(self.start) as f32 / (self.end - self.start) as f32;
        std::array::from_fn(|i| self.from[i] + (to[i] - self.from[i]) * part)
    }
    pub fn retarget(&mut self, law: PanLaw, now: u64, frames: u64) {
        if law == self.target {
            return;
        }
        self.from = self.weights_at(now);
        self.target = law;
        self.start = now;
        self.end = now.saturating_add(frames.max(1));
    }
    #[inline]
    pub fn gains_at(&self, frame: u64, pan: f32) -> (f32, f32) {
        if self.settled(frame) {
            return self.target.gains(pan);
        }
        let weights = self.weights_at(frame);
        let mut out = (0.0, 0.0);
        for (law, weight) in LAWS.into_iter().zip(weights) {
            if weight != 0.0 {
                let (left, right) = law.gains(pan);
                out.0 += weight * left;
                out.1 += weight * right;
            }
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn law_changes_and_interrupted_changes_are_continuous_at_every_pan() {
        let mut ramp = PanLawRamp::at_rest(PanLaw::Balance);
        ramp.retarget(PanLaw::Linear, 100, 10);
        assert_eq!(ramp.gains_at(100, 0.5), (0.5, 1.0));
        assert_eq!(ramp.gains_at(105, 0.5), (0.5, 1.25));
        let before = ramp.gains_at(105, -0.37);
        ramp.retarget(PanLaw::Compromise, 105, 10);
        assert_eq!(ramp.gains_at(105, -0.37), before);
        assert_eq!(ramp.gains_at(115, -0.37), PanLaw::Compromise.gains(-0.37));
    }
}
